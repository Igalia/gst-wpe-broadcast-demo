use gio::{self, prelude::*};
use glib;
use gtk::{self, prelude::*};

use crate::about_dialog::show_about_dialog;
use crate::audio_vumeter;
use crate::header_bar::HeaderBar;
use crate::pipeline::Pipeline;
use crate::settings::show_settings_dialog;
use crate::utils;

use std::cell::RefCell;
use std::error;
use std::ops;
use std::rc::{Rc, Weak};

// Our refcounted application struct for containing all the state we have to carry around.
//
// This represents our main application window.
#[derive(Clone)]
pub struct App(Rc<AppInner>);

// Deref into the contained struct to make usage a bit more ergonomic
impl ops::Deref for App {
    type Target = AppInner;

    fn deref(&self) -> &AppInner {
        &*self.0
    }
}

// Weak reference to our application struct
//
// Weak references are important to prevent reference cycles. Reference cycles are cases where
// struct A references directly or indirectly struct B, and struct B references struct A again
// while both are using reference counting.
pub struct AppWeak(Weak<AppInner>);

impl AppWeak {
    // Upgrade to a strong reference if it still exists
    pub fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

pub struct AppInner {
    main_window: gtk::ApplicationWindow,
    header_bar: HeaderBar,
    pipeline: Pipeline,
    text_view: gtk::TextView,
    css_buffer: RefCell<std::string::String>,
    html_buffer: RefCell<std::string::String>,
    editing_markup: RefCell<Option<std::string::String>>,
    #[allow(dead_code)]
    audio_vumeter: audio_vumeter::AudioVuMeter,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RecordState {
    Idle,
    Recording,
}

impl<'a> From<&'a glib::Variant> for RecordState {
    fn from(v: &glib::Variant) -> RecordState {
        v.get::<bool>().expect("Invalid record state type").into()
    }
}

impl From<bool> for RecordState {
    fn from(v: bool) -> RecordState {
        if v {
            RecordState::Recording
        } else {
            RecordState::Idle
        }
    }
}

impl From<RecordState> for glib::Variant {
    fn from(v: RecordState) -> glib::Variant {
        match v {
            RecordState::Idle => false.to_variant(),
            RecordState::Recording => true.to_variant(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    Settings,
    About,
    Record(RecordState),
    #[allow(dead_code)]
    UpdateOverlay,
}

impl App {
    fn new(application: &gtk::Application) -> Result<App, Box<dyn error::Error>> {
        // Here build the UI but don't show it yet
        let window = gtk::ApplicationWindow::new(application);

        window.set_title("WebCam Viewer");
        window.set_border_width(5);
        window.set_position(gtk::WindowPosition::Center);
        window.set_default_size(1200, -1);

        // Create headerbar for the application window
        let header_bar = HeaderBar::new(&window);

        let vumeter = audio_vumeter::AudioVuMeter::new();

        // Create the pipeline and if that fail return
        let pipeline = Pipeline::new(vumeter.downgrade())
            .map_err(|err| format!("Error creating pipeline: {:?}", err))?;

        let text_view = gtk::TextView::new();
        text_view.set_size_request(400, 300);

        let scrolled_window = gtk::ScrolledWindow::new(gtk::NONE_ADJUSTMENT, gtk::NONE_ADJUSTMENT);
        scrolled_window.set_size_request(400, 300);
        scrolled_window.add(&text_view);

        let css_buffer = RefCell::new(include_str!("../data/style.css").to_string());
        let html_buffer = RefCell::new(include_str!("../data/index.html").to_string());

        let menu = gtk::ComboBoxText::new();

        menu.append_text("CSS");
        menu.append_text("HTML");

        let update_button = gtk::Button::new_with_label("Update web-page overlay");
        update_button
            .clone()
            .upcast::<gtk::Actionable>()
            .set_action_name(Some("app.update_overlay"));

        let vumeter_widget = vumeter.get_widget();
        vumeter_widget.set_size_request(30, -1);

        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        hbox.pack_start(&pipeline.get_widget(), false, false, 0);
        hbox.pack_start(vumeter_widget, false, false, 0);

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.pack_start(&menu, false, false, 0);
        vbox.pack_start(&scrolled_window, true, true, 0);
        vbox.pack_start(&update_button, false, false, 0);

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.pack1(&hbox, false, false);
        paned.pack2(&vbox, false, false);
        paned.set_position(700);

        window.add(&paned);

        let app = App(Rc::new(AppInner {
            main_window: window,
            header_bar,
            pipeline,
            text_view,
            css_buffer,
            html_buffer,
            audio_vumeter: vumeter,
            editing_markup: RefCell::new(None),
        }));

        // Create the application actions
        Action::create(&app, &application);

        let weak_app = app.downgrade();
        menu.connect_changed(move |widget| {
            let app = upgrade_weak!(weak_app);
            if let Some(selection) = widget.get_active_text() {
                if let Some(buffer) = app.text_view.get_buffer() {
                    if selection == "CSS" {
                        buffer.set_text(&*app.css_buffer.borrow());
                    } else {
                        buffer.set_text(&*app.html_buffer.borrow());
                    }
                    app.editing_markup.replace(Some(selection.to_string()));
                }
            }
        });

        menu.set_active(Some(1));

        Ok(app)
    }

    // Downgrade to a weak reference
    pub fn downgrade(&self) -> AppWeak {
        AppWeak(Rc::downgrade(&self.0))
    }

    pub fn on_startup(application: &gtk::Application) {
        // Create application and error out if that fails for whatever reason
        let app = match App::new(application) {
            Ok(app) => app,
            Err(err) => {
                utils::show_error_dialog(
                    true,
                    format!("Error creating application: {}", err).as_str(),
                );
                return;
            }
        };

        // When the application is activated show the UI. This happens when the first process is
        // started, and in the first process whenever a second process is started
        let app_weak = app.downgrade();
        application.connect_activate(move |_| {
            let app = upgrade_weak!(app_weak);
            app.on_activate();
        });

        // When the application is shut down we drop our app struct
        //
        // It has to be stored in a RefCell<Option<T>> to be able to pass it to a Fn closure. With
        // FnOnce this wouldn't be needed and the closure will only be called once, but the
        // bindings define all signal handlers as Fn.
        let app_container = RefCell::new(Some(app));
        application.connect_shutdown(move |_| {
            let app = app_container
                .borrow_mut()
                .take()
                .expect("Shutdown called multiple times");
            app.on_shutdown();
        });
    }

    // Called on the first application instance whenever the first application instance is started,
    // or any future second application instance
    fn on_activate(&self) {
        // Show our window and bring it to the foreground
        self.main_window.show_all();

        // Have to call this instead of present() because of
        // https://gitlab.gnome.org/GNOME/gtk/issues/624
        self.main_window
            .present_with_time((glib::get_monotonic_time() / 1000) as u32);

        // Once the UI is shown, start the GStreamer pipeline. If
        // an error happens, we immediately shut down
        if let Err(err) = self.pipeline.start() {
            utils::show_error_dialog(
                true,
                format!("Failed to set pipeline to playing: {}", err).as_str(),
            );
        }
    }

    // Called when the application shuts down. We drop our app struct here
    fn on_shutdown(self) {
        // This might fail but as we shut down right now anyway this doesn't matter
        // TODO: If a recording is currently running we would like to finish that first
        // before quitting the pipeline and shutting down the pipeline.
        let _ = self.pipeline.stop();
    }

    // When the record button is clicked it triggers the record action, which will call this.
    // We have to start or stop recording here
    fn on_record_state_changed(&self, new_state: RecordState) {
        // Start/stop recording based on button active'ness
        match new_state {
            RecordState::Recording => {
                if let Err(err) = self.pipeline.start_recording() {
                    utils::show_error_dialog(
                        false,
                        format!("Failed to start recording: {}", err).as_str(),
                    );
                    self.header_bar.set_record_active(false);
                }
            }
            RecordState::Idle => self.pipeline.stop_recording(),
        }
    }

    fn update_overlay(&mut self) {
        if let Some(buffer) = self.text_view.get_buffer() {
            if let Some(data) =
                buffer.get_text(&buffer.get_start_iter(), &buffer.get_end_iter(), false)
            {
                if let Some(editing_markup) = &*self.editing_markup.borrow() {
                    if editing_markup == "CSS" {
                        self.css_buffer.replace(data.to_string());
                    } else {
                        self.html_buffer.replace(data.to_string());
                    }
                }
            }
        }
        self.pipeline
            .update_overlay(&self.html_buffer.borrow(), &self.css_buffer.borrow());
    }

    pub fn refresh_pipeline(&self) {
        self.pipeline.refresh();
    }
}

impl Action {
    // The full action name as is used in e.g. menu models
    pub fn full_name(self) -> &'static str {
        match self {
            Action::Quit => "app.quit",
            Action::Settings => "app.settings",
            Action::About => "app.about",
            Action::Record(_) => "app.record",
            Action::UpdateOverlay => "app.update_overlay",
        }
    }

    // Create our application actions here
    //
    // These are connected to our buttons and can be triggered by the buttons, as well as remotely
    fn create(app: &App, application: &gtk::Application) {
        // settings action: when activated, show a settings dialog
        let settings = gio::SimpleAction::new("settings", None);
        let weak_application = application.downgrade();
        let weak_app = app.downgrade();
        settings.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);
            let app = upgrade_weak!(weak_app);

            show_settings_dialog(&application, &app);
        });
        application.add_action(&settings);

        // about action: when activated it will show an about dialog
        let about = gio::SimpleAction::new("about", None);
        let weak_application = application.downgrade();
        about.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);
            show_about_dialog(&application);
        });
        application.add_action(&about);

        // When activated, shuts down the application
        let quit = gio::SimpleAction::new("quit", None);
        let weak_application = application.downgrade();
        quit.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);
            application.quit();
        });
        application.add_action(&quit);

        // And add an accelerator for triggering the action on ctrl+q
        application.set_accels_for_action(Action::Quit.full_name(), &["<Primary>Q"]);

        // record action: changes state between true/false
        let record = gio::SimpleAction::new_stateful("record", None, &RecordState::Idle.into());
        let weak_app = app.downgrade();
        record.connect_change_state(move |action, state| {
            let app = upgrade_weak!(weak_app);
            let state = state.expect("No state provided");
            app.on_record_state_changed(state.into());

            // Let the action store the new state
            action.set_state(state);
        });
        application.add_action(&record);

        // When activated, reload the HTML/CSS data of the overlay
        let update_overlay = gio::SimpleAction::new("update_overlay", None);
        let weak_app = app.downgrade();
        update_overlay.connect_activate(move |_action, _parameter| {
            let mut app = upgrade_weak!(weak_app);
            app.update_overlay();
        });
        application.add_action(&update_overlay);
    }

    // Triggers the provided action on the application
    pub fn trigger<A: IsA<gio::Application> + IsA<gio::ActionGroup>>(self, app: &A) {
        match self {
            Action::Quit => app.activate_action("quit", None),
            Action::Settings => app.activate_action("settings", None),
            Action::About => app.activate_action("about", None),
            Action::Record(new_state) => app.change_action_state("record", &new_state.into()),
            Action::UpdateOverlay => app.activate_action("update_overlay", None),
        }
    }
}
