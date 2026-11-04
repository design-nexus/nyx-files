// SPDX-License-Identifier: MIT


use gtk::prelude::*;

use crate::assets::{self, icons};








/// Search as a field in the header: its icon, a placeholder and its keys.
fn search_field(tooltip: &str, keys: &str) -> gtk::Button {
    let button = gtk::Button::builder().tooltip_text(tooltip).build();
    button.add_css_class("header-search");
    button.set_hexpand(false);
    button.set_valign(gtk::Align::Center);
    button.set_cursor_from_name(Some("text"));
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&assets::chrome_icon(icons::SEARCH));
    let placeholder = gtk::Label::new(Some(&crate::i18n::tr("Search")));
    placeholder.add_css_class("header-search-placeholder");
    placeholder.set_xalign(0.0);
    placeholder.set_hexpand(true);
    row.append(&placeholder);
    let caps = gtk::Label::new(Some(keys));
    caps.add_css_class("header-search-keys");
    caps.set_valign(gtk::Align::Center);
    row.append(&caps);
    button.set_child(Some(&row));
    button
}









