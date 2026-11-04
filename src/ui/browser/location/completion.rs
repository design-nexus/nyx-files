// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{
    glib,
    prelude::*,
};



#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompletionCandidate {
    pub(crate) display_name: String,
    pub(crate) replacement: String,
    pub(crate) parent_hint: String,
    pub(crate) match_len: usize,
}

#[derive(Clone, Copy)]
enum Command {
    Step(i32),
    Down,
    Complete,
    Activate,
}

struct Request {
    generation: u64,
    text: String,
    deferred: Vec<Command>,
    task: glib::JoinHandle<()>,
}

pub(crate) struct PathCompletion {
    popover: gtk::Popover,
    scroll: gtk::ScrolledWindow,
    list: gtk::ListBox,
    candidates: Rc<RefCell<Vec<CompletionCandidate>>>,
    selected_index: Rc<Cell<Option<usize>>>,
    pending_reveal: Cell<bool>,
    suppress_refresh: Cell<bool>,
    is_active: Box<dyn Fn() -> bool>,
    on_activate: Rc<dyn Fn()>,
    generation: Cell<u64>,
    request: RefCell<Option<Request>>,
}

impl Drop for PathCompletion {
    fn drop(&mut self) {
        if self.popover.parent().is_some() {
            self.popover.unparent();
        }
    }
}




impl PathCompletion {

















}









