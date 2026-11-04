// SPDX-License-Identifier: MIT

use std::rc::Rc;

use gtk::prelude::*;

use crate::adapters::bookmarks::{BookmarkWatch, watch_changes};

use super::{SidebarState, show_error_dialog};


#[cfg(test)]
mod tests;
