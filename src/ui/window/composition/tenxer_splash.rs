// SPDX-License-Identifier: MIT

use std::{cell::Cell, rc::Rc, time::Duration};

use gtk::{glib, prelude::*};

use crate::ui::{motion, preferences::PreferenceManager};



#[cfg(test)]
mod tests;
