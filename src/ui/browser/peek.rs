// SPDX-License-Identifier: MIT

use crate::model::{FileEntry, Location};
use crate::ui::browser::ViewState;
use crate::ui::browser::collection::cancel_source;
use crate::ui::browser::entry::{
    entry_icon, icon_for_name, model_display_name, };
use crate::ui::browser::presentation::LoadPresentation;
use crate::ui::browser_modes::BrowserMode;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;





pub(super) struct PeekAnchor {
    pub(super) widget: gtk::Widget,
}

pub(super) struct PeekView {
    pub(super) revealer: gtk::Revealer,
    pub(super) anchor: gtk::Widget,
    pub(super) location: Location,
    pub(super) presentation: LoadPresentation,
    pub(super) model: gtk::StringList,
    pub(super) entries: Rc<RefCell<Vec<FileEntry>>>,
    pub(super) entry_count: Rc<Cell<usize>>,
    pub(super) spinner: gtk::Spinner,
}

#[derive(Clone, Copy)]
pub struct PeekBehavior {
    pub open_delay: Duration,
    pub close_delay: Duration,
    pub fade_duration: Duration,
    pub item_limit: usize,
}

impl Default for PeekBehavior {
    fn default() -> Self {
        Self {
            open_delay: Duration::from_millis(500),
            close_delay: Duration::from_millis(80),
            fade_duration: Duration::from_millis(150),
            item_limit: 8,
        }
    }
}








impl ViewState {




}
