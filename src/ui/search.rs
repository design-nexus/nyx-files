// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::mpsc::TryRecvError,
    time::Duration,
};

use gtk::{gdk, glib, prelude::*};

use super::keys::{
    self,
    bindings::{DOWN, ENTER_ANY_SHIFT, ESCAPE_ANY, OPEN_CONTAINING_FOLDER, UP},
};
use crate::services::{
    NavigationHistory, SearchCoverage, SearchEvent, SearchHandle, SearchItem,
    index_trees_with_exclusions,
};
























#[cfg(test)]
mod tests;
