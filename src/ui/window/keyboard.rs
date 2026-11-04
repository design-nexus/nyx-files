// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk::ModifierType as Modifiers, glib::Propagation, prelude::*};

use crate::{
    app::Browser,
    services::NavigationHistory,
    ui::{
        browser::BrowserView,
        browser_modes::ThumbnailResize,
        keys::{self, Binding, KeyPress, SHORTCUT_MODIFIERS, bindings::*},
        modal::is_closing_layer,
        preview::PreviewDrawer,
        shortcut_footer::ShortcutFooter,
        },
};

use super::{SidebarState, topmost_modal_layer, visible_modal_layer};

pub(crate) mod chooser;
pub(crate) mod chords;
mod commands;
mod escape;
pub(crate) mod files;
mod focus;
mod items;
mod preview;
mod prompts;
mod sidebar;

// None tries the next Files stage; Some(Proceed) gives the event to GTK instead.



#[cfg(test)]
pub(super) fn install(
    window: &impl IsA<gtk::Window>,
    sidebar: &SidebarView,
    parts: DispatcherParts,
) {
    let _ = install_on(window, window.upcast_ref::<gtk::Window>(), sidebar, parts);
}



// Key controllers retain the dispatcher and its window; unrealize breaks the cycle.





















