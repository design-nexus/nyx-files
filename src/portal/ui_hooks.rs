// SPDX-License-Identifier: MIT

//! The window code the portal runs, registered by the interface in use.

use std::sync::{Arc, OnceLock, atomic::AtomicBool};

use ashpd::desktop::file_chooser::SelectedFiles;

use super::ChooserRequest;

pub(crate) type Completion = Box<dyn FnOnce(ashpd::backend::Result<SelectedFiles>)>;

pub(crate) struct ChooserUi {
    pub(crate) prepare: fn(),
    pub(crate) present: fn(ChooserRequest, Arc<AtomicBool>, Completion),
    pub(crate) cancel: fn(&str),
    pub(crate) flush: fn(),
}

static UI: OnceLock<&'static ChooserUi> = OnceLock::new();

pub(crate) fn register(ui: &'static ChooserUi) {
    let _ = UI.set(ui);
}

pub(super) fn ui() -> &'static ChooserUi {
    UI.get().expect("a chooser interface is registered before the portal runs")
}
