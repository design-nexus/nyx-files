// SPDX-License-Identifier: MIT

use std::{
    path::Path,
    time::{Duration, Instant},
};

use gtk::glib;

use super::*;
use crate::{
    model::hidden_names::HiddenName,
    services::file_activity::{self, OperationScope},
    ui::{
        browser::{BrowserView, PeekBehavior},
        browser_modes::BrowserMode,
        preferences::PreferenceManager,
    },
};

fn pump_until(condition: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        glib::MainContext::default().iteration(false);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn widgets<T: IsA<gtk::Widget>>(root: &gtk::Widget, found: &mut Vec<T>) {
    if let Some(widget) = root.downcast_ref::<T>() {
        found.push(widget.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        widgets(&widget, found);
    }
}

fn shown_buttons(window: &gtk::Window, class: &str) -> Vec<gtk::Button> {
    let mut found = Vec::new();
    widgets::<gtk::Button>(window.upcast_ref(), &mut found);
    found
        .into_iter()
        .filter(|button| button.has_css_class(class) && button.is_mapped())
        .collect()
}

fn labelled(window: &gtk::Window, label: &str) -> Option<gtk::Button> {
    let mut found = Vec::new();
    widgets::<gtk::Button>(window.upcast_ref(), &mut found);
    found
        .into_iter()
        .find(|button| button.label().as_deref() == Some(label) && button.is_mapped())
}

fn labelled_count(window: &gtk::Window, label: &str) -> usize {
    let mut found = Vec::new();
    widgets::<gtk::Button>(window.upcast_ref(), &mut found);
    found
        .iter()
        .filter(|button| button.label().as_deref() == Some(label) && button.is_mapped())
        .count()
}

fn shows_text(window: &gtk::Window, text: &str) -> bool {
    let mut labels = Vec::new();
    widgets::<gtk::Label>(window.upcast_ref(), &mut labels);
    let mut inscriptions = Vec::new();
    widgets::<gtk::Inscription>(window.upcast_ref(), &mut inscriptions);
    labels
        .iter()
        .any(|label| label.is_mapped() && label.text().contains(text))
        || inscriptions.iter().any(|inscription| {
            inscription.is_mapped() && inscription.text().is_some_and(|shown| shown.contains(text))
        })
}






