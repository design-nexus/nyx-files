// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use gtk::{glib, prelude::*};

// GTK's own hover delay drops to 60 ms once any tooltip is visible, which would
// flash names across the grid while browsing, so the pointer must rest first.




