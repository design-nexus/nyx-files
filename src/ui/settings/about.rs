// SPDX-License-Identifier: MIT

use gtk::prelude::*;



pub(super) fn toolkit_version() -> String {
    format!(
        "GTK {}.{}.{}",
        gtk::major_version(),
        gtk::minor_version(),
        gtk::micro_version()
    )
}



/// A translated label with a selectable value at the row's end.
pub(super) fn append_detail(container: &gtk::Box, label: &str, value: &str) {
    let row = super::wrap::WrapRow::new(16);
    row.set_end_align(true);
    row.add_css_class("about-detail-row");
    let label = gtk::Label::new(Some(label));
    label.add_css_class("about-detail-label");
    label.add_css_class("settings-control-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    let value = gtk::Label::new(Some(value));
    value.add_css_class("about-detail-value");
    value.add_css_class("settings-nowrap");
    value.set_selectable(true);
    value.set_xalign(1.0);
    row.append(&label);
    row.append(&value);
    container.append(&row);
}
