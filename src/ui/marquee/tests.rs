// SPDX-License-Identifier: MIT

use super::*;


/// The deadline only turns a hang into a deterministic failure.
fn pump_until(ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready() && std::time::Instant::now() < deadline {
        glib::MainContext::default().iteration(false);
    }
}

fn shared_origin_gesture(surface: &gtk::Box) -> gtk::GestureDrag {
    surface
        .observe_controllers()
        .iter::<glib::Object>()
        .flatten()
        .find_map(|controller| controller.downcast::<gtk::GestureDrag>().ok())
        .expect("shared-origin drag gesture")
}



