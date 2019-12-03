use gtk::{self, prelude::*};

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn show_about_dialog(application: &gtk::Application) {
    let dialog = gtk::AboutDialog::new();

    dialog.set_authors(&["Philippe Normand"]);
    dialog.set_website_label(Some("Github repository"));
    dialog.set_website(Some("https://github.com/igalia/gst-wpe-broadcast-demo/"));
    dialog.set_comments(Some(
        "WebCam and Web-page inputs mixed together and streamed to an RTMP end-point.\n \
         \n \
         The code is based on the GUADEC 2019 Rust/GTK/GStreamer workshop app (https://gitlab.gnome.org/sdroege/guadec-workshop-2019). Many thanks to Sebastian Dröge <sebastian@centricular.com> and Guillaume Gomez <guillaume1.gomez@gmail.com>. \n \
         \n \
         The HTML/CSS template is based on the Pure CSS Horizontal Ticker codepen: https://codepen.io/lewismcarey/pen/GJZVoG."
    ));
    dialog.set_copyright(Some("Licensed under MIT license"));
    dialog.set_program_name("GStreamer WPE Broadcast demo");
    dialog.set_logo_icon_name(Some("camera-web"));
    dialog.set_version(Some(VERSION));

    // Make the about dialog modal and transient for our currently active application window. This
    // prevents the user from sending any events to the main window as long as the dialog is open.
    dialog.set_transient_for(application.get_active_window().as_ref());
    dialog.set_modal(true);

    // When any response on the dialog happens, we simply destroy it.
    //
    // We don't have any custom buttons added so this will only ever handle the close button.
    // Otherwise we could distinguish the buttons by the response
    dialog.connect_response(|dialog, _response| {
        dialog.destroy();
    });

    dialog.show_all();
}
