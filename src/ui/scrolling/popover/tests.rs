// SPDX-License-Identifier: MIT

use super::*;




fn scroll_controllers(window: &gtk::Window) -> Vec<gtk::EventControllerScroll> {
    let controllers = window.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|position| {
            controllers
                .item(position)?
                .downcast::<gtk::EventControllerScroll>()
                .ok()
        })
        .collect()
}

