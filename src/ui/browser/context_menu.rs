// SPDX-License-Identifier: MIT

use std::rc::Rc;

mod actions;
mod commands;
mod keyboard;
mod presentation;





// GTK popovers do not shift their anchor to use space across the click point.















pub(in crate::ui) type ContextMenuTrigger = Rc<dyn Fn(f64, f64)>;































