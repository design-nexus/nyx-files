// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gtk::glib;

use super::search_session::{SearchBatch, SearchInput, SearchScope, SearchSession};
use crate::model::lexically_normalized;
pub(crate) use crate::services::RefusedFolders as Refused;

// Avoid crawling intermediate roots while a path is still being typed.






// A colon alone is legal in a filename; don't treat every such name as a URI.











