// SPDX-License-Identifier: MIT

mod click;

use super::*;
use crate::test_support::gtk_test;

#[test]
fn drag_threshold_matches_gtk_on_both_axes() {
    let press = (10.0, 20.0);
    for point in [(18.0, 28.0), (2.0, 12.0), press] {
        assert!(!exceeds_drag_threshold(press, point, 8));
    }
    for point in [(19.0, 20.0), (1.0, 20.0), (10.0, 29.0), (10.0, 11.0)] {
        assert!(exceeds_drag_threshold(press, point, 8));
    }
}






fn pump_until(ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "widget should be allocated"
        );
        glib::MainContext::default().iteration(true);
    }
}
