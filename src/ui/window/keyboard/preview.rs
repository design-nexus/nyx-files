// SPDX-License-Identifier: MIT

//! Preview motion must not move or fill the listing behind the drawer.

use std::rc::Rc;

use gtk::{gdk::ModifierType as Modifiers, glib::Propagation};
use crate::{
    app::Browser,
    ui::{
        keys::{Binding, KeyPress, SHORTCUT_MODIFIERS, bindings::*},
        preview::{preview_target},
        tenxer_mode::Chord,
    },
};





