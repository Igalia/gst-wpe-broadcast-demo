use base64;
use glib;
use gst::{self, prelude::*};
use gtk;
use strfmt::strfmt;

use std::cell::RefCell;
use std::collections::HashMap;
use std::error;
use std::ops;
use std::rc::{Rc, Weak};

use crate::audio_vumeter::AudioVuMeterWeak;
use crate::settings::VideoResolution;
use crate::utils;

// Our refcounted pipeline struct for containing all the media state we have to carry around.
#[derive(Clone)]
pub struct Pipeline(Rc<PipelineInner>);

// Deref into the contained struct to make usage a bit more ergonomic
impl ops::Deref for Pipeline {
    type Target = PipelineInner;

    fn deref(&self) -> &PipelineInner {
        &*self.0
    }
}

pub struct PipelineInner {
    pipeline: gst::Pipeline,
    tee: gst::Element,
    sink: gst::Element,
    wpesrc: gst::Element,
    recording_bin: RefCell<Option<gst::Bin>>,
    recording_audio_pad: RefCell<Option<gst::Pad>>,
    recording_video_pad: RefCell<Option<gst::Pad>>,
    audio_vumeter: AudioVuMeterWeak,
}

// Weak reference to our pipeline struct
//
// Weak references are important to prevent reference cycles. Reference cycles are cases where
// struct A references directly or indirectly struct B, and struct B references struct A again
// while both are using reference counting.
pub struct PipelineWeak(Weak<PipelineInner>);
impl PipelineWeak {
    pub fn upgrade(&self) -> Option<Pipeline> {
        self.0.upgrade().map(Pipeline)
    }
}

fn update_overlay(wpesrc: &gst::Element, html_buffer: &str, css_buffer: &str) {
    const IGALIA_LOGO: &[u8] = include_bytes!("../data/igalia-logo.png");
    let igalia_logo = format!("data:image/png;base64,{}", base64::encode(IGALIA_LOGO));
    let igalia_logo_str = igalia_logo.as_str();

    const GST_LOGO: &[u8] = include_bytes!("../data/gst-logo.svg");
    let gst_logo = format!("data:image/svg+xml;base64,{}", base64::encode(GST_LOGO));
    let gst_logo_str = gst_logo.as_str();

    let mut vars = HashMap::new();
    vars.insert("css_buffer".to_string(), &css_buffer);
    vars.insert("igalia_logo".to_string(), &igalia_logo_str);
    vars.insert("gst_logo".to_string(), &gst_logo_str);

    let data = &strfmt(&html_buffer, &vars).unwrap();
    let bytes = glib::Bytes::from(&data.as_bytes());
    wpesrc.emit("load-bytes", &[&bytes]).unwrap();
}

impl Pipeline {
    pub fn new(audio_vumeter: AudioVuMeterWeak) -> Result<Self, Box<dyn error::Error>> {
        let settings = utils::load_settings();

        let (width, height) = match settings.video_resolution {
            VideoResolution::V480P => (640, 480),
            VideoResolution::V720P => (1280, 720),
            VideoResolution::V1080P => (1920, 1080),
        };

        let pipeline = gst::parse_launch(&format!(
            "glvideomixerelement name=mixer sink_1::zorder=0 sink_1::height={height} sink_1::width={width} \
             ! tee name=tee ! queue ! gtkglsink enable-last-sample=0 name=sink \
             autoaudiosrc ! tee name=audio-tee ! queue ! level ! fakesink sync=1 \
             wpesrc name=wpesrc draw-background=0 ! capsfilter name=wpecaps caps=\"video/x-raw(memory:GLMemory),width={width},height={height},pixel-aspect-ratio=(fraction)1/1\" ! glcolorconvert ! queue ! mixer. \
             v4l2src name=videosrc ! capsfilter name=camcaps caps=\"image/jpeg,width={width},height={height},framerate=30/1\" ! decodebin ! queue ! glupload ! glcolorconvert ! queue ! mixer.", width=width, height=height)
        )?;

        // Upcast to a gst::Pipeline as the above function could've also returned an arbitrary
        // gst::Element if a different string was passed
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("Couldn't downcast pipeline");

        // Request that the pipeline forwards us all messages, even those that it would otherwise
        // aggregate first
        pipeline.set_property_message_forward(true);

        // Retrieve sink and tee elements from the pipeline for later use
        let tee = pipeline.get_by_name("tee").expect("No tee found");
        let sink = pipeline.get_by_name("sink").expect("No sink found");
        let wpesrc = pipeline.get_by_name("wpesrc").expect("No wpesrc found");

        let css_buffer = include_str!("../data/style.css").to_string();
        let html_buffer = include_str!("../data/index.html").to_string();
        update_overlay(&wpesrc, &html_buffer, &css_buffer);

        let pipeline = Pipeline(Rc::new(PipelineInner {
            pipeline,
            tee,
            sink,
            wpesrc,
            audio_vumeter,
            recording_bin: RefCell::new(None),
            recording_audio_pad: RefCell::new(None),
            recording_video_pad: RefCell::new(None),
        }));

        // Install a message handler on the pipeline's bus to catch errors
        let bus = pipeline.pipeline.get_bus().expect("Pipeline had no bus");

        // GStreamer is thread-safe and it is possible to attach bus watches from any thread, which
        // are then nonetheless called from the main thread. So by default, add_watch() requires
        // the passed closure to be Send. We want to pass non-Send values into the closure though.
        //
        // As we are on the main thread and the closure will be called on the main thread, this
        // is actually perfectly fine and safe to do and we can use add_watch_local().
        // add_watch_local() would panic if we were not calling it from the main thread.
        let pipeline_weak = pipeline.downgrade();
        bus.add_watch_local(move |_bus, msg| {
            let pipeline = upgrade_weak!(pipeline_weak, glib::Continue(false));

            pipeline.on_pipeline_message(msg);

            glib::Continue(true)
        })
        .expect("Unable to add bus watch");

        Ok(pipeline)
    }

    pub fn refresh(&self) {
        let settings = utils::load_settings();

        let (width, height) = match settings.video_resolution {
            VideoResolution::V480P => (640, 480),
            VideoResolution::V720P => (1280, 720),
            VideoResolution::V1080P => (1920, 1080),
        };

        let cam_caps_filter = self
            .pipeline
            .get_by_name("camcaps")
            .expect("No webcam capsfilter found");
        let mixer = self.pipeline.get_by_name("mixer").expect("No mixer found");
        let wpecaps_filter = self
            .pipeline
            .get_by_name("wpecaps")
            .expect("No wpe capsfilter found");

        cam_caps_filter.set_property_from_str(
            "caps",
            &format!(
                "image/jpeg,width={width},height={height},framerate=30/1",
                width = width,
                height = height
            ),
        );
        wpecaps_filter.set_property_from_str("caps", &format!("video/x-raw(memory:GLMemory),width={width},height={height},pixel-aspect-ratio=(fraction)1/1", width=width, height=height));

        if let Some(pad) = mixer.get_static_pad("sink_1") {
            pad.set_property("width", &width)
                .expect("No width pad property");
            pad.set_property("height", &height)
                .expect("No height pad property");
        }

        self.pipeline.set_state(gst::State::Paused).unwrap();

        let event = gst::Event::new_reconfigure().build();
        self.sink.send_event(event);

        self.pipeline.set_state(gst::State::Playing).unwrap();
    }

    // Downgrade to a weak reference
    pub fn downgrade(&self) -> PipelineWeak {
        PipelineWeak(Rc::downgrade(&self.0))
    }

    pub fn get_widget(&self) -> gtk::Widget {
        // Get the GTK video sink and retrieve the video display widget from it
        let widget_value = self
            .sink
            .get_property("widget")
            .expect("Sink had no widget property");

        widget_value
            .get::<gtk::Widget>()
            .expect("Sink's widget propery was of the wrong type")
            .unwrap()
    }

    pub fn start(&self) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        // This has no effect if called multiple times
        self.pipeline.set_state(gst::State::Playing)
    }

    pub fn stop(&self) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        // This has no effect if called multiple times
        self.pipeline.set_state(gst::State::Null)
    }

    // Start recording to the configured location
    pub fn start_recording(&self) -> Result<(), Box<dyn error::Error>> {
        let settings = utils::load_settings();

        if settings.rtmp_location.is_none() {
            return Err("Please set the RTMP end-point URL in the settings".into());
        }
        let bin_description = &format!(
            "queue name=video-queue ! gldownload ! videoconvert ! {h264_encoder} ! \
             flvmux streamable=1 name=mux ! rtmpsink enable-last-sample=0 location=\"{location}\" \
             queue name=audio-queue ! fdkaacenc bitrate=128000 ! mux.",
            location = settings.rtmp_location.unwrap(),
            h264_encoder = settings.h264_encoder
        );

        let bin = gst::parse_bin_from_description(bin_description, false)
            .map_err(|err| format!("Failed to create recording pipeline: {}", err))?;
        bin.set_name("recording-bin")
            .map_err(|err| format!("Failed to set recording bin name: {}", err))?;

        let video_queue = bin
            .get_by_name("video-queue")
            .expect("No video-queue found");
        let audio_queue = bin
            .get_by_name("audio-queue")
            .expect("No audio-queue found");
        let audio_tee = self
            .pipeline
            .get_by_name("audio-tee")
            .expect("No audio-tee found");

        // Add the bin to the pipeline. This would only fail if there was
        // already a bin with the same name, which we ensured can't happen
        self.pipeline
            .add(&bin)
            .expect("Failed to add recording bin");

        // Get our tee element by name, request a new source pad from it and then link that to our
        // recording bin to actually start receiving data
        let srcpad = self
            .tee
            .get_request_pad("src_%u")
            .expect("Failed to request new pad from tee");
        let sinkpad = video_queue
            .get_static_pad("sink")
            .expect("Failed to get sink pad from recording bin");

        *self.recording_video_pad.borrow_mut() = Some(srcpad.clone());
        if let Ok(video_ghost_pad) = gst::GhostPad::new(Some("video_sink"), &sinkpad) {
            bin.add_pad(&video_ghost_pad).unwrap();
            // If linking fails, we just undo what we did above
            if let Err(err) = srcpad.link(&video_ghost_pad) {
                // This might fail but we don't care anymore: we're in an error path
                let _ = self.pipeline.remove(&bin);
                let _ = bin.set_state(gst::State::Null);

                return Err(
                    format!("Failed to link recording bin video branch: {}", err)
                        .as_str()
                        .into(),
                );
            }
        }

        let audio_srcpad = audio_tee
            .get_request_pad("src_%u")
            .expect("Failed to request new pad from audio-tee");
        let queue_sinkpad = audio_queue
            .get_static_pad("sink")
            .expect("Failed to get sink pad from queue");

        *self.recording_audio_pad.borrow_mut() = Some(audio_srcpad.clone());
        if let Ok(audio_ghost_pad) = gst::GhostPad::new(Some("audio_sink"), &queue_sinkpad) {
            bin.add_pad(&audio_ghost_pad).unwrap();
            // If linking fails, we just undo what we did above
            if let Err(err) = audio_srcpad.link(&audio_ghost_pad) {
                // This might fail but we don't care anymore: we're in an error path
                let _ = self.pipeline.remove(&bin);
                let _ = bin.set_state(gst::State::Null);

                return Err(
                    format!("Failed to link recording bin audio branch: {}", err)
                        .as_str()
                        .into(),
                );
            }
        }

        bin.set_state(gst::State::Playing)
            .map_err(|_err| "Failed to start recording")?;

        *self.recording_bin.borrow_mut() = Some(bin);

        Ok(())
    }

    // Stop recording if any recording was currently ongoing
    pub fn stop_recording(&self) {
        // Get our recording bin, if it does not exist then nothing has to be stopped actually.
        // This shouldn't really happen
        let bin = match self.recording_bin.borrow_mut().take() {
            None => return,
            Some(bin) => bin,
        };

        let recordind_audio_srcpad = match self.recording_audio_pad.borrow_mut().take() {
            None => return,
            Some(bin) => bin,
        };
        let recordind_video_srcpad = match self.recording_video_pad.borrow_mut().take() {
            None => return,
            Some(bin) => bin,
        };

        let video_queue = bin
            .get_by_name("video-queue")
            .expect("No video-queue found");
        let audio_queue = bin
            .get_by_name("audio-queue")
            .expect("No audio-queue found");

        let sinkpad = video_queue
            .get_static_pad("sink")
            .expect("Failed to get video sink pad from recording bin");

        // Once the tee source pad is idle and we wouldn't interfere with any data flow, unlink the
        // tee and the recording bin and remove/finalize the recording bin
        //
        // The closure below might be called directly from the main UI thread here or at a later
        // time from a GStreamer streaming thread
        let pipeline_weak = self.pipeline.downgrade();
        recordind_video_srcpad.add_probe(gst::PadProbeType::IDLE, move |srcpad, _| {
            // Get the parent of the tee source pad, i.e. the tee itself
            if let Some(parent) = srcpad.get_parent() {
                if let Ok(tee) = parent.downcast::<gst::Element>() {
                    let _ = srcpad.unlink(&sinkpad);
                    tee.release_request_pad(srcpad);

                    let pipeline = upgrade_weak!(pipeline_weak, gst::PadProbeReturn::Remove);
                    pipeline.call_async(move |pipeline| {
                        let bin = match pipeline.get_by_name("recording-bin") {
                            Some(bin) => bin,
                            None => return,
                        };
                        let pbin = pipeline.clone().upcast::<gst::Bin>();
                        // Ignore if the bin was not in the pipeline anymore for whatever
                        // reason. It's not a problem
                        let _ = pbin.remove(&bin);

                        if let Err(err) = bin.set_state(gst::State::Null) {
                            let bus = pbin.get_bus().expect("Pipeline has no bus");
                            let _ = bus.post(&Self::create_application_warning_message(
                                format!("Failed to stop recording: {}", err).as_str(),
                            ));
                        }
                    });

                    // Don't block the pad but remove the probe to let everything
                    // continue as normal
                    return gst::PadProbeReturn::Remove;
                }
            }
            gst::PadProbeReturn::Ok
        });

        let audio_sinkpad = audio_queue
            .get_static_pad("sink")
            .expect("Failed to get audio sink pad from recording bin");

        let pipeline_weak = self.pipeline.downgrade();
        recordind_audio_srcpad.add_probe(gst::PadProbeType::IDLE, move |srcpad, _| {
            // Get the parent of the tee source pad, i.e. the tee itself
            if let Some(parent) = srcpad.get_parent() {
                if let Ok(tee) = parent.downcast::<gst::Element>() {
                    let _ = srcpad.unlink(&audio_sinkpad);
                    tee.release_request_pad(srcpad);

                    let pipeline = upgrade_weak!(pipeline_weak, gst::PadProbeReturn::Remove);
                    pipeline.call_async(move |pipeline| {
                        let bin = match pipeline.get_by_name("recording-bin") {
                            Some(bin) => bin,
                            None => return,
                        };

                        let pbin = pipeline.clone().upcast::<gst::Bin>();
                        // Ignore if the bin was not in the pipeline anymore for whatever
                        // reason. It's not a problem
                        let _ = pbin.remove(&bin);

                        if let Err(err) = bin.set_state(gst::State::Null) {
                            let bus = pbin.get_bus().expect("Pipeline has no bus");
                            let _ = bus.post(&Self::create_application_warning_message(
                                format!("Failed to stop recording: {}", err).as_str(),
                            ));
                        }
                    });

                    // Don't block the pad but remove the probe to let everything
                    // continue as normal
                    return gst::PadProbeReturn::Remove;
                }
            }
            gst::PadProbeReturn::Ok
        });
    }

    pub fn update_overlay(&self, html_buffer: &str, css_buffer: &str) {
        update_overlay(&self.wpesrc, html_buffer, css_buffer);
    }

    // Here we handle all message we get from the GStreamer pipeline. These are notifications sent
    // from GStreamer, including errors that happend at runtime.
    //
    // This is always called from the main application thread by construction.
    fn on_pipeline_message(&self, msg: &gst::MessageRef) {
        use gst::MessageView;

        // A message can contain various kinds of information but
        // here we are only interested in errors so far
        match msg.view() {
            MessageView::Error(err) => {
                utils::show_error_dialog(
                    true,
                    format!(
                        "Error from {:?}: {} ({:?})",
                        err.get_src().map(|s| s.get_path_string()),
                        err.get_error(),
                        err.get_debug()
                    )
                    .as_str(),
                );
            }
            MessageView::Application(msg) => match msg.get_structure() {
                // Here we can send ourselves messages from any thread and show them to the user in
                // the UI in case something goes wrong
                Some(s) if s.get_name() == "warning" => {
                    let text = s
                        .get::<&str>("text")
                        .expect("Warning message without text")
                        .unwrap();
                    utils::show_error_dialog(false, text);
                }
                _ => (),
            },
            MessageView::Element(msg) => {
                if let Some(structure) = msg.get_structure() {
                    if structure.get_name() == "level" {
                        let rms = structure
                            .get::<glib::ValueArray>("rms")
                            .expect("level message without RMS value")
                            .unwrap();
                        let rms_values = rms
                            .iter()
                            .map(|v| v.get_some::<f64>().unwrap())
                            .collect::<Vec<_>>();

                        let peak = structure
                            .get::<glib::ValueArray>("peak")
                            .expect("level message without Peak value")
                            .unwrap();
                        let peak_values = peak
                            .iter()
                            .map(|v| v.get_some::<f64>().unwrap())
                            .collect::<Vec<_>>();

                        let decay = structure
                            .get::<glib::ValueArray>("decay")
                            .expect("level message without Decay value")
                            .unwrap();
                        let decay_values = decay
                            .iter()
                            .map(|v| v.get_some::<f64>().unwrap())
                            .collect::<Vec<_>>();

                        let audio_vumeter = &self.audio_vumeter;
                        let mut vumeter = upgrade_weak!(audio_vumeter);
                        vumeter.update(&rms_values, &peak_values, &decay_values);
                    }
                }
            }
            MessageView::StateChanged(state_changed) => {
                if let Some(element) = msg.get_src() {
                    if element == self.pipeline {
                        let bin_ref = element.downcast_ref::<gst::Bin>().unwrap();
                        let filename = format!(
                            "gst-wpe-broadcast-demo-{:#?}_to_{:#?}",
                            state_changed.get_old(),
                            state_changed.get_current()
                        );
                        bin_ref.debug_to_dot_file_with_ts(gst::DebugGraphDetails::all(), filename);
                    }
                }
            }
            MessageView::AsyncDone(_) => {
                if let Some(element) = msg.get_src() {
                    let bin_ref = element.downcast_ref::<gst::Bin>().unwrap();
                    bin_ref.debug_to_dot_file_with_ts(
                        gst::DebugGraphDetails::all(),
                        "gst-wpe-broadcast-demo-async-done",
                    );
                }
            }
            _ => (),
        };
    }

    fn create_application_warning_message(text: &str) -> gst::Message {
        gst::Message::new_application(
            gst::Structure::builder("warning")
                .field("text", &text)
                .build(),
        )
        .build()
    }
}
