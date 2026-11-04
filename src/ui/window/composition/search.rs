// SPDX-License-Identifier: MIT

use std::{cell::RefCell, rc::Rc};

use gtk::prelude::*;

use crate::{
    app::{Browser, BrowserEvent},
    model::{EntryKind, FileEntry, Location, MetadataValue},
    services::{NavigationHistory, SearchItem},
    ui::{
        browser::BrowserView, preferences::PreferenceManager, preview::PreviewDrawer,
        },
};







