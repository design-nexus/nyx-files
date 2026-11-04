// SPDX-License-Identifier: MIT

//! Exhaustive browser event dispatch. Shared effects and column publication run before alternate
//! presentations consume the event; preserve that order when adding a feature handler.

use crate::ui::browser::ViewState;
use crate::ui::browser_modes::BrowserMode;
use crate::ui::preview::{FOCUS_PREVIEW_DELAY, preview_target};
use gtk::prelude::*;
use gtk::glib;
use std::rc::Rc;


impl ViewState {
















    pub(super) fn mirror_focused_folder(
        self: &Rc<Self>,
        depth: usize,
        position: Option<usize>,
        force: bool,
    ) {
        if let Some(source) = self.pending_mirror.borrow_mut().take() {
            source.remove();
        }
        let Some(position) = position else {
            return;
        };
        if self.browser.child_mirror_suppressed()
            || self.browser.visual_kind().is_some()
            || !self.columns_mirror_selection.get()
            || self.active_rename.borrow().is_some()
            || self.pending_new_entry.borrow().is_some()
            || self.mode_views.borrow().mode() != BrowserMode::Columns
            || (!force
                && self.input_ownership.borrow().last_navigation
                    != crate::ui::input_ownership::NavigationInput::Keyboard)
        {
            return;
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(FOCUS_PREVIEW_DELAY, move || {
            let Some(state) = weak.upgrade() else {
                return;
            };
            state.pending_mirror.borrow_mut().take();
            state.apply_child_mirror(depth, position, force);
        });
        self.pending_mirror.replace(Some(source));
    }

    pub(super) fn apply_child_mirror(self: &Rc<Self>, depth: usize, position: usize, force: bool) {
        let filtered = self
            .columns
            .borrow()
            .get(depth)
            .is_some_and(|column| column.map.has_query());
        // Opening or closing the child column would end a 10xer range. A folder
        // that is still validating will take the focus itself; mirroring it now
        // would cancel that open.
        if filtered
            || self.browser.pending_navigation_generation().is_some()
            || self.browser.child_mirror_suppressed()
            || self.browser.visual_kind().is_some()
            || !self.columns_mirror_selection.get()
            || self.active_rename.borrow().is_some()
            || self.pending_new_entry.borrow().is_some()
            || self.mode_views.borrow().mode() != BrowserMode::Columns
            || (!force
                && self.input_ownership.borrow().last_navigation
                    != crate::ui::input_ownership::NavigationInput::Keyboard)
        {
            return;
        }
        let Some((focused_depth, focused_position, entry)) = self.browser.focused_item() else {
            return;
        };
        if (focused_depth, focused_position) != (depth, position) {
            return;
        }
        if entry.is_directory() {
            self.browser.show_child(depth, entry.location);
        } else {
            self.browser.close_column(depth + 1);
            if self.single_click_previews.get()
                && let Some(entry) = preview_target(Some(entry))
            {
                self.browser.request_automatic_preview(entry);
            }
        }
    }

}
