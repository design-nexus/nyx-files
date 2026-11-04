// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gio, glib, prelude::*};

use crate::{
    model::Location,
    ui::{browser::BrowserView, preferences::PreferenceManager},
};

use super::{
    RailPolicy, RecentAvailability, SIDEBAR_WIDTH, SidebarState, TrashContents,
    };






impl SidebarState {



}

// Trash and reorderable standard places already have a known destination;
// ordinary place rows retain Browser::navigate_location validation.


