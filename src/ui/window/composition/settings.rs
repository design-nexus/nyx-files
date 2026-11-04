// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{glib, prelude::*};

use crate::{
    ui::{
        blur::BlurBin,
        browser::WeakBrowserView,
        keys::{self, bindings::SETTINGS},
        preferences::PreferenceManager,
        settings,
    },
};





