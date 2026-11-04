// SPDX-License-Identifier: MIT

use super::{
    BoundRow, };
use crate::ui::{
    browser::{
        ViewState,
        clipboard::{
            ClipboardMark, clear_active_drag, drag_preview_icon, file_drag_content,
            },
        collection::{ViewMap},
        entry::{
            entry_icon, metadata_needs_fill, model_display_name,
        },
        paths::is_trash_location,
    },
    browser_modes::BrowserMode,
    };
use crate::{
    model::{FileEntry, Location},
    services::SearchItem,
    };
use gtk::{glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};





