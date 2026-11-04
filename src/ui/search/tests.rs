// SPDX-License-Identifier: MIT

use super::*;












fn controller<
    T: glib::object::IsA<gtk::EventController>
        + glib::object::IsA<glib::Object>
        + glib::object::ObjectType,
>(
    widget: &impl IsA<gtk::Widget>,
) -> T {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items())
        .filter_map(|index| controllers.item(index))
        .find_map(|controller| controller.downcast::<T>().ok())
        .expect("expected input controller")
}

fn search_items(prefix: &str, count: usize) -> Vec<SearchItem> {
    (0..count)
        .map(|position| {
            SearchItem::for_test(
                PathBuf::from(format!("/search/{prefix}-{position:03}.txt")),
                false,
            )
        })
        .collect()
}

fn drain_main_context() {
    settle_layout();
}

fn settle_layout() {
    let Some(window) = gtk::Window::list_toplevels()
        .into_iter()
        .find(|window| window.is_mapped())
    else {
        while glib::MainContext::default().iteration(false) {}
        return;
    };
    // Ready GLib sources alone are not a barrier for GTK's frame-clock layout.
    let frames = Rc::new(Cell::new(0));
    let observed = frames.clone();
    window.add_tick_callback(move |_, _| {
        observed.set(observed.get() + 1);
        if observed.get() >= 3 {
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    wait_until(|| frames.get() >= 3);
    wait_until(|| !glib::MainContext::default().iteration(false));
}




fn wait_until(condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "search results timed out"
        );
        glib::MainContext::default().iteration(false);
        std::thread::sleep(Duration::from_millis(1));
    }
}
