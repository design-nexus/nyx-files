// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{glib, prelude::*};

use crate::model::Location;

use super::{
    ViewState,
    
    paths::is_trash_location,
};




