// SPDX-License-Identifier: MIT

use super::*;
use gtk::subclass::prelude::ObjectSubclassIsExt;


fn descendants(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut widgets = Vec::new();
    let mut child = root.first_child();
    while let Some(widget) = child {
        widgets.extend(descendants(&widget));
        child = widget.next_sibling();
        widgets.push(widget);
    }
    widgets
}

fn has_label(root: &gtk::Widget, text: &str) -> bool {
    descendants(root).into_iter().any(|widget| {
        widget
            .downcast::<gtk::Label>()
            .is_ok_and(|label| label.text() == text)
    })
}

fn has_progress_status(root: &gtk::Widget, text: &str) -> bool {
    descendants(root).into_iter().any(|widget| {
        widget.has_css_class("job-status")
            && widget
                .downcast::<gtk::Label>()
                .is_ok_and(|label| label.text() == text)
    })
}

fn wait_until(ready: impl Fn() -> bool) {
    let expired = Rc::new(Cell::new(false));
    let timeout_expired = expired.clone();
    let timeout = glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        timeout_expired.set(true);
    });
    while !ready() && !expired.get() {
        glib::MainContext::default().iteration(true);
    }
    if !expired.get() {
        timeout.remove();
    }
    assert!(ready(), "format feedback did not settle");
}










