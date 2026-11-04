// SPDX-License-Identifier: MIT

use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk::{glib, prelude::*};


struct PendingLoad(RefCell<Option<glib::SourceId>>);

impl PendingLoad {
    fn cancel(&self) {
        if let Some(source) = self.0.borrow_mut().take() {
            source.remove();
        }
    }
}

impl Drop for PendingLoad {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone)]
pub(crate) struct DelayedLoading {
    stack: gtk::Stack,
    pending: Rc<PendingLoad>,
}

impl DelayedLoading {


}


