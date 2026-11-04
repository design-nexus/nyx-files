// SPDX-License-Identifier: MIT

use gtk::prelude::*;
use std::rc::Rc;

mod completion;




pub(super) struct CompactProgress {
    pub(super) root: gtk::Box,
    pub(super) title: gtk::Label,
    pub(super) status: gtk::Label,
    pub(super) info: gtk::Label,
    pub(super) count: gtk::Label,
    pub(super) destination: gtk::Label,
    pub(super) meta: gtk::Label,
    pub(super) progress: gtk::ProgressBar,
    pub(super) cancel: gtk::Button,
    pub(super) complete: gtk::Button,
    pub(super) pin: gtk::ToggleButton,
    icon: gtk::Image,
    completion: Rc<completion::Completion>,
}

pub(super) fn dock(overlay: &gtk::Overlay) -> (gtk::ScrolledWindow, gtk::Box) {
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.has_css_class("file-operation-dock")
            && let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>()
            && let Some(viewport) = scroll.child().and_downcast::<gtk::Viewport>()
            && let Some(list) = viewport.child().and_downcast::<gtk::Box>()
        {
            return (scroll.clone(), list);
        }
    }
    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    // Shadows must fit inside the viewport, not in the scroller's clipped padding.
    list.set_margin_start(16);
    list.set_margin_end(16);
    list.set_margin_top(16);
    list.set_margin_bottom(16);
    let scroll = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .propagate_natural_width(true)
        .max_content_height(320)
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .margin_end(12)
        .margin_bottom(48)
        .build();
    scroll.add_css_class("file-operation-dock");
    overlay.add_overlay(&scroll);
    (scroll, list)
}


impl CompactProgress {






    pub(super) fn remove(&self) {
        self.completion.dismiss();
    }
}
