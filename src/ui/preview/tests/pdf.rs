// SPDX-License-Identifier: MIT

use gtk::gdk::{Key, ModifierType as M};

use super::*;

fn pdf_keys(drawer: &PreviewDrawer) -> gtk::EventControllerKey {
    let mut child = drawer.state.content.first_child();
    while let Some(widget) = child {
        if widget.has_css_class("preview-pdf-scroll") {
            let controllers = widget.observe_controllers();
            return (0..controllers.n_items())
                .find_map(|index| {
                    controllers
                        .item(index)
                        .and_downcast::<gtk::EventControllerKey>()
                })
                .expect("PDF key controller");
        }
        child = widget.next_sibling();
    }
    panic!("no PDF view");
}

fn clipboard_text(clipboard: &gtk::gdk::Clipboard) -> String {
    clipboard
        .content()
        .and_then(|content| content.value(glib::Type::STRING).ok())
        .and_then(|value| value.get::<String>().ok())
        .unwrap_or_default()
}

