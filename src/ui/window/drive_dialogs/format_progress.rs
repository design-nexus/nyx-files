// SPDX-License-Identifier: MIT

use super::drive_ops;
use crate::{
    assets,
    ui::{modal::ModalHost, progress_dock::CompactProgress},
};
use gtk::{glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};


