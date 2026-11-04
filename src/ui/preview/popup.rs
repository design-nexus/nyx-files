// SPDX-License-Identifier: MIT
use super::*;
use crate::ui::preferences::PreviewStyle;
thread_local! {
    static ACTIVE: RefCell<std::rc::Weak<PreviewState>> = const { RefCell::new(std::rc::Weak::new()) };
    static WINDOW: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
}
impl PreviewState {
    pub(super) fn change_presentation(self: &Rc<Self>, style: PreviewStyle) {
        let floating = style == PreviewStyle::Popup;
        if floating == self.floating.get() {
            return;
        }
        let entry = self.current.borrow().clone();
        let depth = self.current_depth.get();
        let was_open = self.is_enabled();
        self.stop();
        self.closing.set(false);
        let previous_window = self.popup.borrow_mut().take();
        if let Some(window) = previous_window {
            window.set_child(None::<&gtk::Widget>);
        }
        if let Some(split) = self.panel_split.borrow().as_ref() {
            split.set_end_child(None::<&gtk::Widget>);
        }
        self.floating.set(floating);
        self.close.set_child(Some(&crate::assets::muted_chrome_icon(if floating {
            crate::assets::icons::X
        } else {
            crate::assets::icons::PANEL_RIGHT_CLOSE
        })));
        self.footer.set_visible(floating);
        if floating {
            self.split.take();
            self.resize_grip.set_visible(false);
            if let Some(browser) = self.sizing.browser() {
                browser.clear_preview_scroll_space();
            }
        } else {
            let split = self.panel_split.borrow().clone();
            if let Some(split) = split.as_ref() {
                split.set_end_child(Some(&self.slot));
            }
            self.split.replace(split);
            self.resize_grip.set_visible(true);
        }
        if was_open && let Some(entry) = entry {
            self.show(entry, depth);
        }
    }

    pub(super) fn present_popup(self: &Rc<Self>) {
        let previous = ACTIVE.with(|active| active.borrow().upgrade());
        if let Some(previous) = previous.filter(|previous| !Rc::ptr_eq(previous, self)) {
            previous.close();
            previous.popup.take();
        }
        ACTIVE.with(|active| *active.borrow_mut() = Rc::downgrade(self));
        let window = WINDOW.with(|stored| {
            if let Some(window) = stored.borrow().as_ref() {
                return window.clone();
            }
            let window = gtk::Window::builder()
                .title(crate::i18n::tr("Preview"))
                .default_width(800)
                .default_height(650)
                .resizable(true)
                .modal(false)
                .build();
            window.add_css_class("strata-window");
            window.add_css_class("preview-popup");
            window.connect_close_request(|window| {
                ACTIVE.with(|active| {
                    if let Some(state) = active.borrow().upgrade() {
                        state.close();
                    }
                });
                window.set_visible(false);
                glib::Propagation::Stop
            });
            let keys_controller = gtk::EventControllerKey::new();
            keys_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
            keys::on_press(&keys_controller, |controller, press| {
                use crate::ui::keys::bindings::{DOWN, ESCAPE, SPACE, UP};
                let Some(state) = ACTIVE.with(|active| active.borrow().upgrade()) else {
                    return glib::Propagation::Proceed;
                };
                if press.is(ESCAPE) {
                    state.close();
                    return glib::Propagation::Stop;
                }
                let editing = controller
                    .widget()
                    .and_then(|w| w.root())
                    .and_then(|r| r.focus())
                    .is_some_and(|w| {
                        w.is::<gtk::Text>()
                            || w.is::<gtk::Entry>()
                            || w.is::<gtk::TextView>()
                            || w.is::<gtk::PasswordEntry>()
                    });
                if editing {
                    return glib::Propagation::Proceed;
                }
                if press.is(SPACE) {
                    state.close();
                    return glib::Propagation::Stop;
                }
                if press.is(UP) || press.is(DOWN) {
                    if let Some(view) = state.sizing.browser() {
                        let browser = view.browser();
                        browser.move_selection(if press.is(UP) { -1 } else { 1 });
                        if let Some(entry) = browser
                            .focused_entry()
                            .and_then(|entry| preview_target(Some(entry)))
                        {
                            state.show(entry, browser.active_depth());
                        }
                    } else if let Some(step) = state.stepper.borrow().clone()
                        && let Some(entry) = step(if press.is(UP) { -1 } else { 1 })
                            .and_then(|entry| preview_target(Some(entry)))
                    {
                        state.show(entry, None);
                    }
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            window.add_controller(keys_controller);
            stored.replace(Some(window.clone()));
            window
        });
        if window.child().as_ref() != Some(self.slot.upcast_ref()) {
            window.set_child(None::<&gtk::Widget>);
            window.set_child(Some(&self.slot));
        }
        if let Some(parent) = self
            .panel_split
            .borrow()
            .as_ref()
            .and_then(|split| split.root())
            .and_downcast::<gtk::Window>()
        {
            window.set_transient_for(Some(&parent));
            for class in parent.css_classes() {
                if class.ends_with("-window") {
                    window.add_css_class(&class);
                }
            }
            // Nyx 2.0 windows have no title bar; the preview header closes it.
            if parent.has_css_class("files-window") && window.titlebar().is_none() {
                window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
            }
            // This cached window is hidden between previews. Registering it with
            // the application would keep Files alive after its last browser closes.
            window.set_application(None::<&gtk::Application>);
        }
        self.popup.replace(Some(window.clone()));
        self.pane.set_size_request(0, -1);
        self.slot.set_hexpand(true);
        self.slot.set_vexpand(true);
        if !window.is_visible() {
            window.present();
        }
    }
}
