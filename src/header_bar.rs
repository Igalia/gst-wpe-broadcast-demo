use gio;
use gtk::{self, prelude::*};

use crate::app::{Action, RecordState};

pub struct HeaderBar {
    record: gtk::ToggleButton,
}

// Create headerbar for the application
//
// This includes the close button and in the future will include also various buttons
impl HeaderBar {
    pub fn new<P: IsA<gtk::Window>>(window: &P) -> Self {
        let header_bar = gtk::HeaderBar::new();

        // Without this the headerbar will have no close button
        header_bar.set_show_close_button(true);

        // Create a menu button with the hamburger menu
        let main_menu = gtk::MenuButton::new();
        let main_menu_image =
            gtk::Image::new_from_icon_name(Some("open-menu-symbolic"), gtk::IconSize::Menu);
        main_menu.set_image(Some(&main_menu_image));

        // Create the menu model with the menu items. These directly activate our application
        // actions by their name
        let main_menu_model = gio::Menu::new();
        main_menu_model.append(Some("Settings"), Some(Action::Settings.full_name()));
        main_menu_model.append(Some("About"), Some(Action::About.full_name()));
        main_menu.set_menu_model(Some(&main_menu_model));

        // And place it on the right (end) side of the header bar
        header_bar.pack_end(&main_menu);

        // Create record button and let it trigger the record action
        let record_button = gtk::ToggleButton::new();
        let record_button_image =
            gtk::Image::new_from_icon_name(Some("network-cellular"), gtk::IconSize::Menu);
        record_button.set_image(Some(&record_button_image));

        record_button.connect_toggled(|record_button| {
            let app = gio::Application::get_default().expect("No default application");
            Action::Record(RecordState::from(record_button.get_active())).trigger(&app);
        });

        // Place the record button on the left
        header_bar.pack_start(&record_button);

        // Insert the headerbar as titlebar into the window
        window.set_titlebar(Some(&header_bar));

        HeaderBar {
            record: record_button,
        }
    }

    pub fn set_record_active(&self, active: bool) {
        self.record.set_active(active);
    }
}
