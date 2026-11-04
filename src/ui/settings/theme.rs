// SPDX-License-Identifier: MIT

use std::rc::Rc;

use gtk::{gio, prelude::*};

use crate::{
    assets::icons,
    ui::{
        preferences::{InterfaceRenderer, PreferenceManager},
        theme::ThemeManager,
    },
};

use super::{
    bindings::{bind_switch},
    page_content, scrollable_page,
};







