// SPDX-License-Identifier: MIT

use crate::{
    test_support::gtk_test,
    ui::{preferences::TextSize, theme::ThemeManager},
};
use gtk::{glib, prelude::*};

fn settle() {
    let main_loop = glib::MainLoop::new(None, false);
    let stop = main_loop.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || stop.quit());
    main_loop.run();
}

