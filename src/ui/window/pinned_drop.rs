// SPDX-License-Identifier: MIT

#[cfg(test)]
mod tests;

use std::{cell::RefCell, future::Future, rc::Rc, time::Duration};

use gtk::{gdk, glib, prelude::*};

use super::SidebarState;
use crate::{
    model::Location,
    ui::browser::{paths::is_trash_location},
};

















