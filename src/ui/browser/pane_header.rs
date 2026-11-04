// SPDX-License-Identifier: MIT

use crate::app::Browser;
use crate::model::{FolderSort, SortDirection, SortKey};
use crate::ui::browser::ViewState;
use crate::ui::keys::{self, bindings::BACKSPACE};
use crate::ui::preferences::PreferenceManager;
use gtk::gdk::ModifierType as Modifiers;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;










