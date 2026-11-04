// SPDX-License-Identifier: MIT

use std::{cell::RefCell, rc::Rc};

use gtk::{gio, prelude::*};

use crate::ui::{
    blur::BlurBin, browser::BrowserView, preferences::PreferenceManager, preview::PreviewDrawer,
};

use super::{keyboard};

mod input;
mod layout;
mod search;
mod settings;
mod tabs;
mod tenxer_splash;




