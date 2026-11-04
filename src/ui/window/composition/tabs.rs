// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gio, glib, prelude::*};

use crate::{
    adapters::location_for_file,
    app::Presence,
    model::Location,
    ui::{
        blur::BlurBin,
        browser::BrowserView,
        keys::{self, KeyPress, bindings::*},
        preferences::PreferenceManager,
        tabs_session,
    },
};

use super::{layout, tenxer_splash};

mod strip;











