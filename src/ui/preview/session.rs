// SPDX-License-Identifier: MIT

use super::*;

impl PreviewDrawer {
    pub fn is_enabled(&self) -> bool {
        self.state.is_enabled()
    }



    pub(crate) fn clear_target(&self) {
        self.state.clear_target();
    }
}

impl PreviewState {
    pub(super) fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    pub(super) fn set_enabled(&self, enabled: bool) -> bool {
        let previous = self.enabled.replace(enabled);
        self.refresh_panel_action();
        previous
    }

    pub(super) fn refresh_panel_action(&self) {
        self.enabled_action
            .set_state(&self.is_enabled().to_variant());
    }

    pub(super) fn toggle_panel(self: &Rc<Self>, entry: Option<FileEntry>, depth: Option<usize>) {
        if self.is_enabled() {
            self.dismissed.set(true);
            self.close();
        } else {
            self.toggle(entry, depth);
        }
    }

    pub(super) fn toggle(self: &Rc<Self>, entry: Option<FileEntry>, depth: Option<usize>) {
        if self.is_enabled() {
            self.close();
        } else {
            self.dismissed.set(false);
            self.set_enabled(true);
            self.focus_archive_on_ready.set(true);
            if let Some(entry) = entry.and_then(|entry| preview_target(Some(entry))) {
                self.show(entry, depth);
            } else {
                self.clear_target();
            }
        }
    }

    pub(super) fn clear_target(&self) {
        self.reset_find();
        self.continue_playback.take();
        self.cancel_pending_show();
        self.claim_on_resume.set(false);
        self.focus_archive_on_ready.set(false);
        self.animating.set(false);
        self.sizing.close();
        self.animation_generation
            .set(self.animation_generation.get().saturating_add(1));
        self.current_request.set(None);
        self.current_depth.set(None);
        self.current.borrow_mut().take();
        self.load.borrow_mut().take();
        self.clear_raw_details();
        self.cancel_loading();
        self.pdf_loads.borrow_mut().clear();
        self.clear_content();
        // An open popup keeps its window and header while the selection has
        // nothing to show, so Esc and stepping still work from it.
        let popup_open = self.floating.get()
            && self.is_enabled()
            && self.popup.borrow().as_ref().is_some_and(|window| window.is_visible());
        if popup_open {
            self.show_placeholder();
        } else {
            self.hide_panel();
        }
    }

    pub(super) fn show_placeholder(&self) {
        if self
            .content
            .first_child()
            .is_some_and(|child| child.has_css_class("preview-placeholder"))
        {
            return;
        }
        self.clear_content();
        self.title.set_text(&crate::i18n::tr(PREVIEW_LABEL));
        crate::ui::accessibility::set_description(&self.title, None);
        self.icon.set_visible(false);
        self.metadata.set_visible(false);
        self.open.set_sensitive(false);
        self.header_handle.set_cursor_from_name(None);
        let placeholder = gtk::Label::builder()
            .label(crate::i18n::tr("No preview for this selection"))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .hexpand(true)
            .vexpand(true)
            .margin_start(24)
            .margin_end(24)
            .build();
        placeholder.add_css_class("preview-placeholder");
        placeholder.add_css_class("preview-feedback-detail");
        self.content.append(&placeholder);
    }
}
