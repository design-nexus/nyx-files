// SPDX-License-Identifier: MIT

use std::{collections::HashMap, rc::Rc};

use gtk::prelude::*;

use super::*;
use crate::app::BrowserEvent;
use crate::services::{OperationEvent, OperationRequestId, PasteItem, TransferConflict};
use crate::test_support::operations::HeldOperations;
use crate::ui::browser::{BrowserView, PeekBehavior};

fn renamed(count: usize) -> TransferReport {
    TransferReport {
        renamed: (0..count)
            .map(|index| RenamedItem {
                requested: Location::local(format!("/copy/item {index}")),
                saved_as: Location::local(format!("/copy/item {index} (1)")),
                cause: RenameCause::Alias,
            })
            .collect(),
        ..TransferReport::default()
    }
}

#[test]
fn a_name_fixed_for_its_control_characters_shows_where_they_were() {
    let item = RenamedItem {
        requested: Location::local("/stick/line\none\u{1}\u{85}.txt"),
        saved_as: Location::local("/stick/line_one__.txt"),
        cause: RenameCause::Invalid,
    };

    assert_eq!(
        renamed_line(&item),
        rust_i18n::t!(
            "“%{name}” was saved as “%{new_name}”",
            name = "line\u{fffd}one\u{fffd}\u{fffd}.txt",
            new_name = "line_one__.txt"
        )
    );
}





#[test]
fn a_stop_lists_renamed_items_by_cause_under_its_outcome() {
    let outcome = "Could not copy keys: Permission denied";
    let mut report = renamed(LISTED_ITEMS + 2);
    report.renamed[0].cause = RenameCause::Taken;

    let detail = outcome_detail(outcome.to_owned(), &report);

    assert!(detail.starts_with(&format!("{outcome}\n\n")), "{detail}");
    for heading in [
        "The destination can\u{2019}t tell these names apart",
        "This name was already taken",
    ] {
        let heading = crate::i18n::tr(heading);
        assert_eq!(detail.matches(&heading).count(), 1, "{heading}");
    }
    assert!(detail.contains(&renamed_line(&report.renamed[0])));
    assert_eq!(detail.matches('•').count(), 1 + LISTED_ITEMS);
    assert!(detail.ends_with(&crate::i18n::count("more_items", 1)));
}

fn shows(overlay: &gtk::Overlay, class: &str) -> bool {
    shows_widget(overlay, |widget| widget.has_css_class(class))
}

fn shows_text(overlay: &gtk::Overlay, text: &str) -> bool {
    shows_widget(overlay, |widget| {
        widget
            .downcast_ref::<gtk::Label>()
            .is_some_and(|label| label.label() == text)
    })
}

fn shows_widget(overlay: &gtk::Overlay, found: impl Fn(&gtk::Widget) -> bool) -> bool {
    let mut stack = vec![overlay.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = stack.pop() {
        if found(&widget) {
            return true;
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            stack.push(next);
        }
    }
    false
}


fn modal_layers(window: &gtk::Window) -> Vec<gtk::Widget> {
    let mut found = Vec::new();
    let mut stack = vec![window.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = stack.pop() {
        if widget.has_css_class("app-modal-layer") {
            found.push(widget);
            continue;
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            stack.push(next);
        }
    }
    found
}

fn dialogs(window: &gtk::Window) -> Vec<Vec<String>> {
    modal_layers(window)
        .iter()
        .filter(|layer| !crate::ui::modal::is_closing_layer(layer))
        .map(texts)
        .collect()
}

fn texts(root: &gtk::Widget) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            found.push(label.label().to_string());
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            stack.push(next);
        }
    }
    found
}

fn pump_until(done: impl Fn() -> bool) {
    let context = gtk::glib::MainContext::default();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        while context.pending() {
            context.iteration(false);
        }
        if done() {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Started {
    Move,
    Copy,
    Restore,
    MergeUndo,
    Extract,
}

fn start(view: &BrowserView, operations: &HeldOperations, started: Started) -> OperationRequestId {
    let browser = view.browser();
    match started {
        Started::Move | Started::Copy => browser.transfer(
            Location::local("/destination"),
            vec![PasteItem {
                source: Location::local("/source/run"),
                conflict: TransferConflict::FailIfExists,
                target_name: None,
            }],
            started == Started::Move,
            false,
        ),
        Started::Restore => browser.restore(vec![crate::services::RestoreTrashItem {
            entry: crate::test_support::operations::entry(Location::local("/trash/files/run")),
            destination: "/source/run".into(),
        }]),
        Started::MergeUndo => {
            let copy = start(view, operations, Started::Copy);
            operations.emit(
                copy,
                OperationEvent::Merged {
                    request_id: copy,
                    source: Location::local("/source/run"),
                    created: vec![Location::local("/destination/run/app.conf")],
                    overwritten: Vec::new(),
                    placed: HashMap::new(),
                },
            );
            operations.emit(
                copy,
                OperationEvent::Pasted {
                    request_id: copy,
                    locations: Vec::new(),
                },
            );
            let (generation, created, overwritten) = browser
                .pending_undo_merge()
                .expect("the merged copy's undo");
            assert!(browser.undo_merge(generation, created, overwritten));
        }
        Started::Extract => {
            browser.extract(
                crate::test_support::operations::entry(Location::local("/source/run.tar")),
                Location::local("/destination"),
                false,
                None,
            );
            let id = browser.last_started_operation().expect("the extraction");
            operations.emit(
                id,
                OperationEvent::ArchiveStarted {
                    request_id: id,
                    total: 0,
                },
            );
        }
    }
    browser.last_started_operation().expect("an operation")
}

