// SPDX-License-Identifier: MIT


use gtk::prelude::*;


















pub(crate) fn abbreviate_home(path: &std::path::Path) -> String {
    let home = glib::home_dir();
    if let Ok(rest) = path.strip_prefix(&home) {
        format!("~/{}", rest.display())
    } else {
        path.display().to_string()
    }
}














