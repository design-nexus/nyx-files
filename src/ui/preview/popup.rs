// SPDX-License-Identifier: MIT
use super::*;
use crate::ui::keys::bindings::{DOWN, ESCAPE, PEEK, SPACE, UP};
use crate::ui::preferences::PreviewStyle;

thread_local! {
    /// The preview whose popup is showing; opening another tab's closes it.
    static ACTIVE: RefCell<std::rc::Weak<PreviewState>> = const { RefCell::new(std::rc::Weak::new()) };
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
        self.destroy_popup();
        if let Some(split) = self.panel_split.borrow().as_ref() {
            split.set_end_child(None::<&gtk::Widget>);
        }
        self.floating.set(floating);
        self.place_header(floating);
        self.close.set_icon_name(if floating {
            "window-close-symbolic"
        } else {
            "sidebar-show-right-symbolic"
        });
        self.footer.set_visible(floating);
        if floating {
            self.split.take();
            self.resize_grip.set_visible(false);
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

    /// A floating preview's header moves its window; a docked one stays plain.
    fn place_header(&self, floating: bool) {
        let framed = self.header.parent().as_ref() == Some(self.header_frame.upcast_ref());
        if framed == floating {
            return;
        }
        if floating {
            self.pane.remove(&self.header);
            self.header_frame.set_child(Some(&self.header));
            self.pane.prepend(&self.header_frame);
        } else {
            self.header_frame.set_child(None::<&gtk::Widget>);
            self.pane.remove(&self.header_frame);
            self.pane.prepend(&self.header);
        }
    }

    fn popup_window(self: &Rc<Self>) -> gtk::Window {
        if let Some(window) = self.popup.borrow().as_ref() {
            return window.clone();
        }
        let window = gtk::Window::builder()
            .title(crate::i18n::tr("Preview"))
            .default_width(800)
            .default_height(650)
            .resizable(true)
            .modal(false)
            .build();
        window.add_css_class("files-window");
        window.add_css_class("preview-popup");
        // The preview header is the window's header.
        window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        // A hidden popup registered with the application would keep Files
        // running after its last browser window closes.
        window.set_application(None::<&gtk::Application>);
        nyx_ui::chrome::follow_placement(&window, |_| {});
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |window| {
            if let Some(state) = weak.upgrade() {
                state.close();
            }
            window.set_visible(false);
            glib::Propagation::Stop
        });
        let keys_controller = gtk::EventControllerKey::new();
        keys_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys::on_press(&keys_controller, move |controller, press| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if state.popup_key(controller, press) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        window.add_controller(keys_controller);
        window.set_child(Some(&self.slot));
        self.popup.replace(Some(window.clone()));
        window
    }

    fn popup_key(self: &Rc<Self>, controller: &gtk::EventControllerKey, press: &KeyPress) -> bool {
        if press.is(ESCAPE) {
            if !self.close_find_if_open() {
                self.close();
            }
            return true;
        }
        let editing = controller
            .widget()
            .and_then(|widget| widget.root())
            .and_then(|root| root.focus())
            .is_some_and(|focused| super::super::focus_navigation::accepts_typing(&focused));
        if editing {
            return false;
        }
        let tenxer = super::super::preferences::PreferenceManager::shared().tenxer_mode();
        if press.is(SPACE) || (tenxer && press.is(PEEK)) {
            self.close();
            return true;
        }
        if Self::is_find_key(press) {
            return self.open_find();
        }
        if press.is(UP) || press.is(DOWN) {
            self.step_listing(if press.is(UP) { -1 } else { 1 });
            return true;
        }
        let command = media_key_command(press);
        command.is_some_and(|command| self.has_media_view() && self.media_command(command))
    }

    pub(super) fn present_popup(self: &Rc<Self>) {
        let previous = ACTIVE.with(|active| active.borrow().upgrade());
        if let Some(previous) = previous.filter(|previous| !Rc::ptr_eq(previous, self)) {
            previous.close();
        }
        ACTIVE.with(|active| *active.borrow_mut() = Rc::downgrade(self));
        let window = self.popup_window();
        if let Some(parent) = self
            .panel_split
            .borrow()
            .as_ref()
            .and_then(|split| split.root())
            .and_downcast::<gtk::Window>()
        {
            window.set_transient_for(Some(&parent));
        }
        self.pane.set_size_request(0, -1);
        self.slot.set_hexpand(true);
        self.slot.set_vexpand(true);
        if !window.is_visible() {
            window.present();
        }
    }

    /// Names the popup after the file it shows.
    pub(super) fn title_popup(&self, name: &str) {
        if let Some(window) = self.popup.borrow().as_ref() {
            nyx_ui::chrome::set_title(window, &crate::i18n::tr("Files"), name);
        }
    }

    pub(super) fn destroy_popup(&self) {
        // Taken first: unrealizing the pane reaches `stop`, which reads `popup`.
        let window = self.popup.borrow_mut().take();
        if let Some(window) = window {
            window.set_child(None::<&gtk::Widget>);
            window.destroy();
        }
    }
}
