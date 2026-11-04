// SPDX-License-Identifier: MIT

use crate::ui::browser::ViewState;
use crate::ui::scrolling::advance;
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};




// Scroller-relative coordinates stay fixed as content moves beneath the drag.
pub(in crate::ui::browser) struct DragAutoscroll {
    state: Weak<ViewState>,
    controller: glib::WeakRef<gtk::DropControllerMotion>,
    pointer: Cell<(f64, f64)>,
    burst_x: Cell<Option<(Instant, i8)>>,
    burst_y: Cell<Option<(Instant, i8)>>,
    frame_time: Cell<i64>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
    // GTK does not re-evaluate drop targets when content scrolls under a parked
    // pointer, so the autoscroll tracks the hovered column itself.
    hovered: RefCell<Option<gtk::Box>>,
}

impl ViewState {
    // Blank strip space has no drop target, so track motion on the scroller.
}

impl DragAutoscroll {








}




