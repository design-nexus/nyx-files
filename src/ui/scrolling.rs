// SPDX-License-Identifier: MIT

//! Scrolling shared by collection views: middle-click autoscroll, page-sized
//! keyboard navigation, and wheel routing for transient browser panels.

use gtk::prelude::*;

pub(super) mod popover;

pub(super) fn advance(adjustment: &gtk::Adjustment, step: f64) {
    if step == 0.0 {
        return;
    }
    let upper = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
    let target = (adjustment.value() + step).clamp(adjustment.lower(), upper);
    if (target - adjustment.value()).abs() >= f64::EPSILON {
        adjustment.set_value(target);
    }
}

#[cfg(test)]
mod tests;
