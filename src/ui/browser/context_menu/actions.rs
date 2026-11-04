// SPDX-License-Identifier: MIT

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::{Rc, Weak},
};

use gtk::{gio, prelude::*};



use crate::{
    assets::{self, icons},
    model::{FileEntry, Location, MenuPlacement},
    services::{InvocationSource, MatchedAction},
    ui::actions::{action_icon, folder_input, inputs_for_entries, native_paths, run_action},
};

use super::super::{ViewState};





// GTK before 4.22 can keep a window's pointer focus on a hidden popover and crash
// on its missing surface after unparenting. A sensitivity change repicks the target.

// GTK before 4.22 leaves each replaced model's position handler on its freed
// section box, so the popover must start without a model.






// Preserve generated rows: GTK uses them to own submenu selection and grabs.




