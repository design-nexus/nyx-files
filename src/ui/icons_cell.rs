// SPDX-License-Identifier: MIT

use gtk::prelude::*;

mod layout;
mod name_tooltip;

pub(super) const MIN_ICONS_THUMBNAIL_SIZE: i32 = 32;
pub(super) const MAX_ICONS_THUMBNAIL_SIZE: i32 = 256;
const ICONS_CARD_LABEL_CHARS: i32 = 12;
const ICONS_CARD_LABEL_LINES: i32 = 2;
const ICONS_CARD_LABEL_LINE_PX: i32 = 18;
const ICONS_CARD_CAPTION_GAP: i32 = 4;




pub(super) fn rename_field(card: &impl IsA<gtk::Widget>) -> Option<gtk::Entry> {
    let labels = card.last_child()?.downcast::<gtk::Box>().ok()?;
    let mut sibling = labels.first_child();
    while let Some(widget) = sibling {
        sibling = widget.next_sibling();
        if let Ok(entry) = widget.downcast::<gtk::Entry>() {
            return Some(entry);
        }
    }
    None
}

pub(super) fn ensure_rename_field(card: &impl IsA<gtk::Widget>) -> Option<gtk::Entry> {
    if let Some(field) = rename_field(card) {
        return Some(field);
    }
    let labels = card.last_child()?.downcast::<gtk::Box>().ok()?;
    let field = gtk::Entry::new();
    field.add_css_class("inline-rename");
    crate::ui::accessibility::set_label(&field, &crate::i18n::tr("Rename"));
    field.set_width_chars(1);
    field.set_hexpand(true);
    gtk::prelude::EntryExt::set_alignment(&field, 0.5);
    field.set_visible(false);
    labels.append(&field);
    Some(field)
}






