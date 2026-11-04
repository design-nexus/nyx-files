// SPDX-License-Identifier: MIT

use std::rc::Rc;

use gtk::{
    gdk::{Key, ModifierType as Modifiers},
    glib::Propagation,
    prelude::*,
};
use crate::{
    app::{Browser, VisualKind},
    model::Location,
    ui::{
        browser_modes::{BrowserMode, ThumbnailResize},
        keys::{Binding, KeyPress, bindings::*},
        preview::{MediaCommand, preview_target},
        tenxer_mode::Chord,
        window::{
            home_directory, },
    },
};






