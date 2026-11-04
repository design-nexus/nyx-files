// SPDX-License-Identifier: MIT

use std::rc::Rc;

use gtk::{
    glib::{self, Propagation},
    prelude::*,
};
use crate::{
    app::Browser,
    ui::{
        browser::BrowserView,
        keys::{SHORTCUT_MODIFIERS, bindings::*},
        
    },
};


