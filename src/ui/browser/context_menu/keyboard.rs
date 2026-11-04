// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk::ModifierType as Modifiers, glib, prelude::*};

use crate::ui::keys::{
    self,
    bindings::{
        DOWN, ENTER_ANY_SHIFT, ESCAPE_ANY_SHIFT, FIRST_ITEM, FOCUS_NEXT, FOCUS_PREVIOUS, LAST_ITEM,
        SPACE, UP,
    },
};








