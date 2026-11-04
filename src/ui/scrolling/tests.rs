// SPDX-License-Identifier: MIT

use super::*;

fn wait_for_allocation(widget: &gtk::Widget) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while widget.width() <= 0 || widget.height() <= 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "widget was not allocated"
        );
        glib::MainContext::default().iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn middle_gesture(scroll: &gtk::ScrolledWindow) -> gtk::GestureClick {
    let controllers = scroll.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index).and_downcast::<gtk::GestureClick>())
        .find(|click| click.button() == gtk::gdk::BUTTON_MIDDLE)
        .expect("middle-click autoscroll gesture")
}

fn press(gesture: &gtk::GestureClick, x: f64, y: f64) {
    gesture.emit_by_name::<()>("pressed", &[&1i32, &x, &y]);
}

