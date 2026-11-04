// SPDX-License-Identifier: GPL-3.0-or-later
use crate::model::FileEntry;
use crate::ui::browser::entry::entry_icon;
use crate::ui::modal::window_overlay;
use gtk::glib;
use gtk::prelude::*;
use std::time::Duration;




#[derive(Clone)]
struct Flyer {
    widget: gtk::Overlay,
    icon: gtk::Image,
    start: (f64, f64),
    end: (f64, f64),
    arc_height: f64,
    bank_class: &'static str,
    delay: Duration,
    target_name: Option<String>,
    burst_center: Option<(f64, f64)>,
    index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flight {
    Inbound,
    Outbound,
    Release,
}

pub(super) struct PreparedFlight {
    overlay: gtk::Overlay,
    source: gtk::Widget,
    trash_button: gtk::Button,
    flyers: Option<Vec<Flyer>>,
    frozen_root: Option<crate::ui::blur::BlurBin>,
    mode: Flight,
}

impl Drop for PreparedFlight {
    fn drop(&mut self) {
        if let Some(flyers) = self.flyers.take() {
            for flyer in flyers {
                self.overlay.remove_overlay(&flyer.widget);
            }
        }
        if let Some(root) = self.frozen_root.take() {
            root.thaw();
        }
    }
}

impl PreparedFlight {
}





// The destination is outside the Trash view; fly out rather than toward disappearing rows.














