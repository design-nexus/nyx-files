use gtk::prelude::*;

mod delay;
mod surface;
pub(crate) use delay::DelayedLoading;
pub(crate) use surface::{focus_surface_or, surface_takes_focus};

/// Directory stack pages shared by the panes and columns.
pub(crate) const CONTENT_PAGE: &str = "content";






