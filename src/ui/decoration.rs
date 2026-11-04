// SPDX-License-Identifier: MIT

use gtk::prelude::*;


/// GTK only builds its default title bar at realize time for windows it does
/// not already consider client-decorated.
#[cfg(test)]
pub(crate) fn assert_default_decoration_absent(window: &impl IsA<gtk::Window>) {
    let window = window.as_ref();
    assert!(window.is_realized(), "window must be realized");
    assert!(
        window.has_css_class("csd") || window.has_css_class("solid-csd"),
        "GTK must treat the window as client-decorated"
    );
    let titlebar = window.titlebar().expect("registered titlebar");
    assert!(
        !titlebar.is_visible(),
        "the placeholder titlebar takes no space"
    );
    let mut child = window.first_child();
    while let Some(widget) = child {
        assert!(
            !widget.has_css_class("default-decoration"),
            "GTK added its default title bar"
        );
        child = widget.next_sibling();
    }
}


