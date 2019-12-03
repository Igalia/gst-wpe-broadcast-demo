use gtk::{self, prelude::*};

use crate::app::App;
use crate::utils;

use std::cell::RefCell;
use std::fs::create_dir_all;
use std::ops;
use std::rc::{Rc, Weak};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
pub enum VideoResolution {
    V480P,
    V720P,
    V1080P,
}

// Convenience for converting from the strings in the combobox
impl From<Option<glib::GString>> for VideoResolution {
    fn from(s: Option<glib::GString>) -> Self {
        if let Some(s) = s {
            match s.to_lowercase().as_str() {
                "480p" => VideoResolution::V480P,
                "720p" => VideoResolution::V720P,
                "1080p" => VideoResolution::V1080P,
                _ => panic!("unsupported video resolution {}", s),
            }
        } else {
            VideoResolution::default()
        }
    }
}

impl Default for VideoResolution {
    fn default() -> Self {
        VideoResolution::V720P
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Settings {
    pub rtmp_location: Option<std::string::String>,
    pub h264_encoder: std::string::String,
    pub video_resolution: VideoResolution,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            rtmp_location: None,
            h264_encoder: "video/x-raw,format=NV12 ! vaapih264enc bitrate=20000 keyframe-period=60 ! video/x-h264,profile=main".to_string(),
            video_resolution: VideoResolution::default(),
        }
    }
}

// Our refcounted settings struct for containing all the widgets we have to carry around.
//
// This represents our settings dialog.
#[derive(Clone)]
struct SettingsDialog(Rc<SettingsDialogInner>);

// Deref into the contained struct to make usage a bit more ergonomic
impl ops::Deref for SettingsDialog {
    type Target = SettingsDialogInner;

    fn deref(&self) -> &SettingsDialogInner {
        &*self.0
    }
}

// Weak reference to our settings dialog struct
//
// Weak references are important to prevent reference cycles. Reference cycles are cases where
// struct A references directly or indirectly struct B, and struct B references struct A again
// while both are using reference counting.
struct SettingsDialogWeak(Weak<SettingsDialogInner>);

impl SettingsDialogWeak {
    // Upgrade to a strong reference if it still exists
    pub fn upgrade(&self) -> Option<SettingsDialog> {
        self.0.upgrade().map(SettingsDialog)
    }
}

struct SettingsDialogInner {
    rtmp_location: gtk::Entry,
    h264_encoder: gtk::Entry,
    video_resolution: gtk::ComboBoxText,
}

impl SettingsDialog {
    // Downgrade to a weak reference
    fn downgrade(&self) -> SettingsDialogWeak {
        SettingsDialogWeak(Rc::downgrade(&self.0))
    }

    // Take current settings value from all our widgets and store into the configuration file
    fn save_settings(&self) {
        let h264_encoder = match self.h264_encoder.get_text() {
            Some(e) => e,
            None => {
                utils::show_error_dialog(false, "Please specify an H.264 encoder chain");
                return;
            }
        };

        let rtmp_location = match self.rtmp_location.get_text() {
            Some(l) => Some(l.into()),
            None => None,
        };

        let settings = Settings {
            rtmp_location,
            h264_encoder: h264_encoder.to_string(),
            video_resolution: VideoResolution::from(self.video_resolution.get_active_text()),
        };

        utils::save_settings(&settings);
    }
}

// Construct the settings dialog and ensure that the settings file exists and is loaded
pub fn show_settings_dialog(application: &gtk::Application, app: &App) {
    let s = utils::get_settings_file_path();

    if !s.exists() {
        if let Some(parent_dir) = s.parent() {
            if !parent_dir.exists() {
                if let Err(e) = create_dir_all(parent_dir) {
                    utils::show_error_dialog(
                        false,
                        format!(
                            "Error while trying to build settings snapshot_directory '{}': {}",
                            parent_dir.display(),
                            e
                        )
                        .as_str(),
                    );
                }
            }
        }
    }

    let settings = utils::load_settings();

    // Create an empty dialog with close button
    let dialog = gtk::Dialog::new_with_buttons(
        Some("WPE overlay broadcast settings"),
        application.get_active_window().as_ref(),
        gtk::DialogFlags::MODAL,
        &[("Close", gtk::ResponseType::Close)],
    );

    // All the UI widgets are going to be stored in a grid
    let grid = gtk::Grid::new();
    grid.set_column_spacing(4);
    grid.set_row_spacing(4);
    grid.set_margin_bottom(12);

    let resolution_label = gtk::Label::new(Some("Video resolution"));
    let video_resolution = gtk::ComboBoxText::new();

    resolution_label.set_halign(gtk::Align::Start);

    video_resolution.append_text("480P");
    video_resolution.append_text("720P");
    video_resolution.append_text("1080P");
    video_resolution.set_active(match settings.video_resolution {
        VideoResolution::V480P => Some(0),
        VideoResolution::V720P => Some(1),
        VideoResolution::V1080P => Some(2),
    });
    video_resolution.set_hexpand(true);

    grid.attach(&resolution_label, 0, 1, 1, 1);
    grid.attach(&video_resolution, 1, 1, 3, 1);

    let rtmp_label = gtk::Label::new(Some("RTMP end-point URL"));
    let rtmp_location = gtk::Entry::new();
    if let Some(location) = settings.rtmp_location {
        rtmp_location.set_text(&location);
    }

    rtmp_label.set_halign(gtk::Align::Start);

    grid.attach(&rtmp_label, 0, 3, 1, 1);
    grid.attach(&rtmp_location, 1, 3, 3, 1);

    let encoder_label = gtk::Label::new(Some("H.264 encoder"));
    let h264_encoder = gtk::Entry::new();
    h264_encoder.set_text(&settings.h264_encoder);

    encoder_label.set_halign(gtk::Align::Start);

    grid.attach(&encoder_label, 0, 4, 1, 1);
    grid.attach(&h264_encoder, 1, 4, 3, 1);

    // Put the grid into the dialog's content area
    let content_area = dialog.get_content_area();
    content_area.pack_start(&grid, true, true, 0);
    content_area.set_border_width(10);

    let settings_dialog = SettingsDialog(Rc::new(SettingsDialogInner {
        rtmp_location,
        h264_encoder,
        video_resolution,
    }));

    let settings_dialog_weak = settings_dialog.downgrade();
    settings_dialog
        .rtmp_location
        .connect_property_text_notify(move |_| {
            let settings_dialog = upgrade_weak!(settings_dialog_weak);
            settings_dialog.save_settings();
        });

    let settings_dialog_weak = settings_dialog.downgrade();
    settings_dialog
        .h264_encoder
        .connect_property_text_notify(move |_| {
            let settings_dialog = upgrade_weak!(settings_dialog_weak);
            settings_dialog.save_settings();
        });

    let settings_dialog_weak = settings_dialog.downgrade();
    let weak_app = app.downgrade();
    settings_dialog.video_resolution.connect_changed(move |_| {
        let settings_dialog = upgrade_weak!(settings_dialog_weak);
        settings_dialog.save_settings();
        let app = upgrade_weak!(weak_app);
        app.refresh_pipeline();
    });

    // Close the dialog when the close button is clicked. We don't need to save the settings here
    // as we already did that whenever the user changed something in the UI.
    //
    // The closure keeps the one and only strong reference to our settings dialog struct and it
    // will be freed once the dialog is destroyed
    let settings_dialog_storage = RefCell::new(Some(settings_dialog));
    let weak_app = app.downgrade();
    dialog.connect_response(move |dialog, _| {
        dialog.destroy();

        let _ = settings_dialog_storage.borrow_mut().take();
        let app = upgrade_weak!(weak_app);
        app.refresh_pipeline();
    });

    dialog.set_resizable(false);
    dialog.show_all();
}
