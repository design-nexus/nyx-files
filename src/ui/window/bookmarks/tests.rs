// SPDX-License-Identifier: MIT

use std::{cell::RefCell, rc::Rc};

use gtk::glib;

use crate::{
    model::Location,
    storage::writer,
    test_support::{save_pins, saved_pins},
};

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !condition() {
        assert!(std::time::Instant::now() < deadline, "operation timed out");
        glib::MainContext::default().iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}




