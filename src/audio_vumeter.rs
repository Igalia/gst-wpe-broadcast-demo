// Ported from Voctomix's Python audio level widget:
// https://github.com/voc/voctomix/blob/master/voctogui/lib/audioleveldisplay.py

use cairo;
use gtk::{self, prelude::*};
use num;

use std::cell::RefCell;
use std::ops;
use std::rc::{Rc, Weak};

#[derive(Clone)]
pub struct AudioVuMeter(Rc<AudioVuMeterInner>);

impl ops::Deref for AudioVuMeter {
    type Target = AudioVuMeterInner;

    fn deref(&self) -> &AudioVuMeterInner {
        &*self.0
    }
}

#[derive(Debug)]
struct LevelData {
    rms: Vec<f64>,
    peak: Vec<f64>,
    decay: Vec<f64>,
}

pub struct AudioVuMeterInner {
    drawing_area: gtk::DrawingArea,
    data: RefCell<Option<LevelData>>,
    cached_height: RefCell<Option<i32>>,
    bg_lg: RefCell<Option<cairo::LinearGradient>>,
    rms_lg: RefCell<Option<cairo::LinearGradient>>,
    peak_lg: RefCell<Option<cairo::LinearGradient>>,
    decay_lg: RefCell<Option<cairo::LinearGradient>>,
}

pub struct AudioVuMeterWeak(Weak<AudioVuMeterInner>);
impl AudioVuMeterWeak {
    pub fn upgrade(&self) -> Option<AudioVuMeter> {
        self.0.upgrade().map(AudioVuMeter)
    }
}

impl AudioVuMeter {
    pub fn new() -> Self {
        let vumeter = AudioVuMeter(Rc::new(AudioVuMeterInner {
            drawing_area: gtk::DrawingArea::new(),
            data: RefCell::new(None),
            cached_height: RefCell::new(None),
            bg_lg: RefCell::new(None),
            rms_lg: RefCell::new(None),
            peak_lg: RefCell::new(None),
            decay_lg: RefCell::new(None),
        }));

        let vumeter_weak = vumeter.downgrade();
        let area = vumeter.get_widget();
        area.connect_draw(move |_, cr| {
            if let Some(mut vumeter) = vumeter_weak.upgrade() {
                vumeter.on_draw(cr)
            } else {
                Inhibit(false)
            }
        });

        vumeter
    }

    pub fn downgrade(&self) -> AudioVuMeterWeak {
        AudioVuMeterWeak(Rc::downgrade(&self.0))
    }

    pub fn get_widget(&self) -> &gtk::DrawingArea {
        &self.0.drawing_area
    }

    pub fn update(&mut self, rms: &[f64], peak: &[f64], decay: &[f64]) {
        *self.0.data.borrow_mut() = Some(LevelData {
            rms: rms.to_vec(),
            peak: peak.to_vec(),
            decay: decay.to_vec(),
        });
        self.0.drawing_area.queue_draw();
    }

    fn on_draw(&mut self, cr: &cairo::Context) -> Inhibit {
        let area = &self.0.drawing_area;
        let width = area.get_allocated_width();
        let height = area.get_allocated_height();

        let update_gradients = match *self.cached_height.borrow() {
            Some(h) => h != height,
            None => true,
        };

        if update_gradients {
            *self.cached_height.borrow_mut() = Some(height);
            // setup gradients for all level bars
            *self.bg_lg.borrow_mut() = Some(self.gradient(0.25, 0.0, height.into()));
            *self.rms_lg.borrow_mut() = Some(self.gradient(1.0, 0.0, height.into()));
            *self.peak_lg.borrow_mut() = Some(self.gradient(0.75, 0.0, height.into()));
            *self.decay_lg.borrow_mut() = Some(self.gradient(1.0, 0.5, height.into()));
        }

        if let Some(data) = &*self.0.data.borrow() {
            let channels = data.rms.len() as i32;

            // space between the channels in px
            let margin = 2;

            // 1 channel -> 0 margins, 2 channels -> 1 margin, 3 channels…
            let channel_width = (width - (margin * (channels - 1))) / channels;

            let height_float = f64::from(height);

            // normalize db-value to 0…1 and multiply with the height
            let rms_px = data
                .rms
                .iter()
                .map(|db| self.normalize_db(*db) * height_float)
                .collect::<Vec<_>>();
            let peak_px = data
                .peak
                .iter()
                .map(|db| self.normalize_db(*db) * height_float)
                .collect::<Vec<_>>();
            let decay_px = data
                .decay
                .iter()
                .map(|db| self.normalize_db(*db) * height_float)
                .collect::<Vec<_>>();

            for channel in 0..channels {
                // start-coordinate for this channel
                let x = (channel * channel_width) + (channel * margin);
                let channel_idx = channel as usize;

                // draw background
                cr.rectangle(
                    x.into(),
                    0.0,
                    channel_width.into(),
                    height_float - peak_px[channel_idx],
                );

                if let Some(gradient) = self.bg_lg.borrow().as_ref() {
                    cr.set_source(gradient);
                    cr.fill();
                }

                // draw peak bar
                cr.rectangle(
                    x.into(),
                    height_float - peak_px[channel_idx],
                    channel_width.into(),
                    peak_px[channel_idx],
                );
                if let Some(gradient) = self.peak_lg.borrow().as_ref() {
                    cr.set_source(gradient);
                    cr.fill();
                }

                // draw rms bar below
                cr.rectangle(
                    x.into(),
                    height_float - rms_px[channel_idx],
                    channel_width.into(),
                    rms_px[channel_idx] - peak_px[channel_idx],
                );
                if let Some(gradient) = self.rms_lg.borrow().as_ref() {
                    cr.set_source(gradient);
                    cr.fill();
                }

                // draw decay bar
                cr.rectangle(
                    x.into(),
                    height_float - decay_px[channel_idx],
                    channel_width.into(),
                    2.0,
                );
                if let Some(gradient) = self.decay_lg.borrow().as_ref() {
                    cr.set_source(gradient);
                    cr.fill();
                }

                // draw medium grey margin bar
                if margin > 0 {
                    cr.rectangle(
                        f64::from(x) + f64::from(channel_width),
                        0.0,
                        margin.into(),
                        height.into(),
                    );
                    cr.set_source_rgb(0.5, 0.5, 0.5);
                    cr.fill();
                }
            }

            for db in [-40, -20, -10, -5, -4, -3, -2, -1].iter() {
                let text = format!("{}", db);
                let extents = cr.text_extents(&text);
                let textwidth = extents.width;
                let textheight = extents.height;

                let y = self.normalize_db(f64::from(*db)) * height_float;
                if y > peak_px[channels as usize - 1] {
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                } else {
                    cr.set_source_rgb(0.0, 0.0, 0.0);
                }

                cr.move_to(
                    (f64::from(width) - textwidth) - 2.0,
                    height_float - y - textheight,
                );
                cr.show_text(&text);
            }
            Inhibit(true)
        } else {
            Inhibit(false)
        }
    }

    fn normalize_db(&self, db: f64) -> f64 {
        // -60db -> 1.00 (very quiet)
        // -30db -> 0.75
        // -15db -> 0.50
        //  -5db -> 0.25
        //  -0db -> 0.00 (very loud)
        let val = -0.15 * db + 1.0;
        let logscale = 1.0 - val.log10();
        num::clamp(logscale, 0.0, 1.0)
    }

    fn gradient(&self, brightness: f64, darkness: f64, height: f64) -> cairo::LinearGradient {
        let lg = cairo::LinearGradient::new(0.0, 0.0, 0.0, height);
        lg.add_color_stop_rgb(0.0, brightness, darkness, darkness);
        lg.add_color_stop_rgb(0.22, brightness, brightness, darkness);
        lg.add_color_stop_rgb(0.25, brightness, brightness, darkness);
        lg.add_color_stop_rgb(0.35, darkness, brightness, darkness);
        lg.add_color_stop_rgb(1.0, darkness, brightness, darkness);
        lg
    }
}
