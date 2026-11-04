// SPDX-License-Identifier: MIT

use super::*;
use crate::ui::{blur::BlurBin, preferences::PreferenceManager};




fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut widgets = vec![widget.clone()];
    for child in children(widget) {
        widgets.extend(descendants(&child));
    }
    widgets
}

fn item(layer: &gtk::Widget, id: &str) -> gtk::Widget {
    descendants(layer)
        .into_iter()
        .find(|widget| tagged(widget, id))
        .expect("searchable setting")
}

fn tagged(widget: &gtk::Widget, id: &str) -> bool {
    widget.widget_name() == format!("settings-search-{id}")
}





fn nav(layer: &gtk::Widget, page: &str) -> gtk::Button {
    descendants(layer)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
        .find(|button| button.widget_name() == page)
        .unwrap_or_else(|| panic!("navigation button for {page}"))
}

/// Whether `widget` and every ancestor up to its stack page are visible.
fn shown_on_page(widget: &gtk::Widget) -> bool {
    let mut current = Some(widget.clone());
    while let Some(widget) = current {
        if widget
            .parent()
            .is_some_and(|parent| parent.is::<gtk::Stack>())
        {
            return true;
        }
        if !widget.is_visible() {
            return false;
        }
        current = widget.parent();
    }
    false
}


fn tagged_or_inside_tag(widget: &gtk::Widget, page: &gtk::Widget) -> bool {
    let mut current = Some(widget.clone());
    while let Some(widget) = current {
        if widget.widget_name().starts_with("settings-search-") {
            return true;
        }
        if &widget == page {
            return false;
        }
        current = widget.parent();
    }
    false
}

fn describe(widget: &gtk::Widget) -> String {
    let text = descendants(widget)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk::Label>().ok())
        .map(|label| label.text().to_string())
        .find(|text| !text.is_empty())
        .unwrap_or_default();
    format!("{} {text:?}", widget.type_().name())
}

