// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use gtk::{glib, prelude::*};

use super::{
    BoundModeItem, ClickActivation, ListColumnLayout, PaneSection, SourceIndexMap,
    TransferHandlerSlot, };
use crate::{
    app::Browser,
    model::FileEntry,
    ui::{
        accessibility,
        browser::{self, ClipboardMark, ClipboardMarks},
        thumbnail,
    },
};







