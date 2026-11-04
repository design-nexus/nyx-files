// SPDX-License-Identifier: MIT

use super::chooser_context;
use crate::adapters::gio_file_for_location;
use crate::model::{FileEntry, Location};
use crate::services::ArchiveFormat;
use crate::ui::browser::desktop::{launch_terminal};
use crate::ui::browser::entry::{entry_icon};
use crate::ui::browser::paths::{
    is_trash_location,
};
use crate::ui::browser::{PinStatus, ViewState};
use crate::ui::browser_modes::BrowserMode;
use crate::ui::preferences::PreferenceManager;
use crate::ui::shortcut_reference::{self};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

mod actions;
mod commands;
mod keyboard;
mod presentation;





// GTK popovers do not shift their anchor to use space across the click point.















pub(in crate::ui) type ContextMenuTrigger = Rc<dyn Fn(f64, f64)>;































