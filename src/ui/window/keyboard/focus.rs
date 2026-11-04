// SPDX-License-Identifier: MIT

use gtk::{gdk::ModifierType as Modifiers, glib::Propagation, prelude::*};
use crate::{
    app::Browser,
    ui::{
        browser_modes::BrowserMode,
        focus_navigation::{
            arrow_direction, },
        keys::{SHORTCUT_MODIFIERS, bindings::*},
        },
};

