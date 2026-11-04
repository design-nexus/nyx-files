// SPDX-License-Identifier: MIT

use super::*;
use std::cell::Cell;


fn assert_focus(window: &gtk::Window, widget: &impl IsA<gtk::Widget>) {
    assert!(
        gtk::prelude::RootExt::focus(window)
            .is_some_and(|focused| { focused == *widget.as_ref() || focused.is_ancestor(widget) })
    );
}

fn settle() {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < until {
        while glib::MainContext::default().iteration(false) {}
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
