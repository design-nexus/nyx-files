// SPDX-License-Identifier: MIT

use std::rc::Rc;

use gtk::prelude::*;

use crate::ui::loading_skeleton::{CONTENT_PAGE};




#[derive(Clone)]
pub(super) struct LoadPresentation {
    pub(super) stack: gtk::Stack,
    loading: crate::ui::loading_skeleton::DelayedLoading,
    message: gtk::Label,
    retry: Option<gtk::Button>,
}

impl LoadPresentation {








}

