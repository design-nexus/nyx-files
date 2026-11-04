// SPDX-License-Identifier: MIT

use std::{cell::Cell, rc::Rc};

use gtk::{gdk, prelude::*};

use crate::app::Presence;





#[cfg(test)]
fn follow_power_saver(_: &gtk::Window, _: Rc<Cell<bool>>, _: Update) {}


#[cfg(test)]
mod tests;
