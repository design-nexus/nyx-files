// SPDX-License-Identifier: MIT

use std::ffi::OsStr;

use super::{FontRendering, font_rendering, turns_portals_off, xft_dpi};

#[test]
fn portals_are_off_only_for_the_no_portals_flag() {
    assert!(turns_portals_off(OsStr::new("no-portals")));
    assert!(turns_portals_off(OsStr::new("events,no-portals")));
    assert!(!turns_portals_off(OsStr::new("")));
    assert!(!turns_portals_off(OsStr::new("events")));
    assert!(!turns_portals_off(OsStr::new("no-portals-please")));
}

#[test]
fn font_settings_become_the_xft_values_gdk_gives_gtk() {
    assert_eq!(
        font_rendering("grayscale", "slight", "rgb"),
        FontRendering { antialias: 1, hinting: 1, hint_style: "hintslight", rgba: "none" }
    );
    assert_eq!(
        font_rendering("rgba", "full", "vbgr"),
        FontRendering { antialias: 1, hinting: 1, hint_style: "hintfull", rgba: "vbgr" }
    );
    assert_eq!(
        font_rendering("none", "none", "rgb"),
        FontRendering { antialias: 0, hinting: 0, hint_style: "hintnone", rgba: "none" }
    );
}

#[test]
fn text_scaling_becomes_xft_dpi() {
    assert_eq!(xft_dpi(1.0), 98304);
    assert_eq!(xft_dpi(1.25), 122880);
}
