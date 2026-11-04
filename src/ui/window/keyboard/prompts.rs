// SPDX-License-Identifier: MIT

//! 10xer footer prompts. While a prompt has focus only its own keys act; every
//! other key edits the text and never reaches a browsing command.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::{
    gdk::ModifierType as Modifiers,
    glib::{self, Propagation},
};
use crate::{
    app::Browser,
    model::{FileEntry, Location},
    services::NavigationHistory,
    ui::{
        browser::{BrowserView},
        folder_picker::{self, Refused},
        keys::{KeyPress, bindings::*},
        shortcut_footer::{CandidateKeys, ShortcutFooter},
        tenxer_mode::Prompt,
    },
};













