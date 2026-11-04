// SPDX-License-Identifier: MIT

//! 10xer mode: Files' keyboard-first, Yazi-style keys. Plain letters move and
//! act, and g , c ; t start chords the footer shows. Prompts open in the
//! footer.

use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use crate::{
    model::{FileEntry, Location},
    services::TransferConflict,
    ui::{
        keys::{self, KeyPress, Keys, bindings::*},
        preferences::PreferenceManager,
        tenxer_mode::{Chord, Prompt},
        window::keyboard::{
            chords::{GoTarget, go_target},
            files::{action_slot, slot_key, sort_choice},
        },
    },
};

use super::{actions, menus, tab::Tab, window::Window};

#[derive(Default)]
pub(super) struct State {
    /// Where visual selection started, while it is on.
    anchor: Cell<Option<u32>>,
    last_find: RefCell<Option<(String, bool)>>,
    /// What an open Rename, Move to, Copy to or Extract to prompt acts on.
    targets: RefCell<Vec<FileEntry>>,
}

pub(super) fn enabled() -> bool {
    PreferenceManager::shared().tenxer_mode()
}

fn tr(text: &str) -> String {
    crate::i18n::tr(text)
}

/// Connects a tab's footer: prompts submit and filter as typed, and the
/// sidebar shows place keys while **g** waits.
pub(super) fn wire(window: &Rc<Window>, tab: &Rc<Tab>) {
    let (w, t) = (Rc::downgrade(window), Rc::downgrade(tab));
    tab.footer.connect_prompt_activated(move || {
        if let (Some(w), Some(t)) = (w.upgrade(), t.upgrade()) {
            submit_prompt(&w.tenxer, Some(&w), &t);
        }
    });
    let t = Rc::downgrade(tab);
    tab.footer.connect_prompt_changed(move |kind, text| {
        let Some(tab) = t.upgrade() else { return };
        match kind {
            Prompt::Filter => tab.set_search(&text, false),
            Prompt::Search => tab.set_search(&text, true),
            _ => {}
        }
    });
}

/// Escape in a prompt: Filter clears; Search keeps its results; others close.
pub(super) fn cancel_prompt(tab: &Rc<Tab>) {
    let kind = tab.footer.open_prompt_kind();
    tab.footer.dismiss_prompt();
    if kind == Some(Prompt::Filter) {
        tab.set_search("", false);
    }
    tab.focus_view();
}

fn feedback(tab: &Tab, text: &str) {
    tab.footer.show_feedback(&tr(text));
}

/// The 10xer keys of a browser window; true when one was handled.
pub(super) fn handle(window: &Rc<Window>, tab: &Rc<Tab>, press: &KeyPress) -> bool {
    let pins = window.visible_pins();
    if let Some(chord) = tab.footer.armed_chord()
        && matches!(chord, Chord::Copy | Chord::Action | Chord::Tabs)
    {
        tab.footer.cancel_chord();
        if press.is(SETTINGS) {
            return false;
        }
        if !press.is(ESCAPE) && !window_chord(window, tab, chord, press) {
            feedback(tab, "Unknown chord");
        }
        return true;
    }
    if tab.footer.armed_chord().is_none() && file_key(window, tab, press) {
        return true;
    }
    browse(&window.tenxer, tab, press, &pins)
}

/// Keys that change files, open other windows or reach the sidebar.
fn file_key(window: &Rc<Window>, tab: &Rc<Tab>, press: &KeyPress) -> bool {
    let state = &window.tenxer;
    let selected = tab.selected();
    if press.is_any(&[YANK, YANK_CUT]) {
        let cut = press.is(YANK_CUT);
        if selected.is_empty() {
            feedback(
                tab,
                if cut {
                    "Nothing to cut"
                } else {
                    "Nothing to yank"
                },
            );
        } else {
            actions::copy(&selected, cut);
        }
    } else if press.is_any(&[UNYANK, UNYANK_X]) {
        actions::clear_cut();
    } else if press.is_any(&[PASTE_KEEP_BOTH, PASTE_REPLACE]) {
        let preset = if press.is(PASTE_REPLACE) {
            TransferConflict::ReplaceExisting
        } else {
            TransferConflict::KeepBoth
        };
        if let Some(here) = tab.location() {
            actions::paste_with(window, tab, here, Some(preset));
        }
    } else if press.is_any(&[TENXER_DELETE, DELETE]) {
        if selected.is_empty() {
            feedback(tab, "Nothing to delete");
        } else {
            menus::remove(window, tab, selected);
        }
    } else if press.is_any(&[TENXER_DELETE_PERMANENTLY, DELETE_PERMANENTLY]) {
        if selected.is_empty() {
            feedback(tab, "Nothing to delete");
        } else {
            menus::delete_permanently(window, tab, selected);
        }
    } else if press.is(CREATE) {
        if tab.location().is_some_and(|l| l.native_path().is_some()) {
            tab.footer.open_prompt(Prompt::Create);
        } else {
            feedback(tab, "Can’t create items here");
        }
    } else if press.is_any(&[TENXER_RENAME, RENAME]) {
        open_rename(state, tab, selected);
    } else if press.is(OPEN_WITH) {
        if selected.is_empty() {
            feedback(tab, "Nothing to open");
        } else {
            super::open_with::show_dialog(window, selected);
        }
    } else if press.is_any(&[MOVE_TO, COPY_TO]) {
        let kind = if press.is(MOVE_TO) {
            Prompt::MoveTo
        } else {
            Prompt::CopyTo
        };
        destination_prompt(state, tab, kind, selected);
    } else if press.is(RESTORE) {
        if selected.is_empty() {
            feedback(tab, "Nothing to restore");
        } else {
            menus::restore(window, tab, selected);
        }
    } else if press.is_any(&[JUMP_VISITED, JUMP_RECENT]) {
        super::search::open(window, super::search::Mode::Folders);
    } else if press.is(TENXER_SIDEBAR) {
        window.focus_sidebar();
    } else if press.is(COPY_AS) {
        tab.footer.arm_chord(Chord::Copy);
    } else if press.is(ACTIONS) {
        arm_actions(tab);
    } else if press.is(TAB_CHORD) {
        tab.footer.arm_chord(Chord::Tabs);
    } else {
        return false;
    }
    true
}

/// Moving, finding, sorting and the g chord: the keys every 10xer listing
/// has, the chooser's too. `pins` are the sidebar's pins in order.
pub(super) fn browse(state: &State, tab: &Rc<Tab>, press: &KeyPress, pins: &[Location]) -> bool {
    if let Some(chord) = tab.footer.armed_chord() {
        tab.footer.cancel_chord();
        if press.is(SETTINGS) {
            return false;
        }
        if !press.is(ESCAPE) && !browse_chord(state, tab, chord, press, pins) {
            feedback(tab, "Unknown chord");
        }
        return true;
    }
    let anchor = state.anchor.get();
    let cursor = tab.cursor().map(i64::from);
    let at = cursor.unwrap_or(0);
    let selected = tab.selected();
    let browser = &tab.browser;
    let page = tab.page_rows();
    if press.is_any(&[VIM_DOWN, DOWN]) {
        tab.move_cursor(cursor.map_or(0, |c| c + 1), anchor);
    } else if press.is_any(&[VIM_UP, UP]) {
        tab.move_cursor(cursor.map_or(0, |c| c - 1), anchor);
    } else if press.is_any(&[VIM_LEFT, LEFT]) {
        tab.step_out();
    } else if press.is_any(&[VIM_RIGHT, RIGHT])
        && let Some(entry) = selected.last().filter(|e| !e.is_directory())
    {
        // A file opens in the drawer, which takes the keys; it never launches.
        match crate::ui::preview::preview_target(Some(entry.clone())) {
            Some(target) => {
                tab.preview.show(target, browser.active_depth());
                tab.preview.take_keyboard();
            }
            None => feedback(tab, "Nothing to preview"),
        }
    } else if press.is_any(&[VIM_RIGHT, RIGHT, ENTER]) {
        tab.step_in();
    } else if press.is_any(&[SCROLL_PREVIEW_DOWN, SCROLL_PREVIEW_UP]) {
        if tab.preview.is_open() {
            let lines = if press.is(SCROLL_PREVIEW_DOWN) { 1 } else { -1 };
            tab.preview
                .scroll_document(crate::ui::preview::DocumentScroll::Line(lines));
        }
    } else if press.is(TENXER_LAST_ITEM) {
        tab.move_cursor(i64::MAX, anchor);
    } else if press.is(HALF_PAGE_DOWN) {
        tab.move_cursor(at + page / 2, anchor);
    } else if press.is(HALF_PAGE_UP) {
        tab.move_cursor(at - page / 2, anchor);
    } else if press.is_any(&[TENXER_PAGE_DOWN, TENXER_PAGE_DOWN_PAGE_DOWN]) {
        tab.move_cursor(at + page, anchor);
    } else if press.is_any(&[TENXER_PAGE_UP, TENXER_PAGE_UP_PAGE_UP]) {
        tab.move_cursor(at - page, anchor);
    } else if press.is(OPEN) {
        if selected.is_empty() {
            feedback(tab, "Nothing to open");
        }
        selected.iter().for_each(|e| tab.open(e));
    } else if press.is(PEEK) {
        tab.toggle_preview();
    } else if press.is(TENXER_HISTORY_BACK) {
        browser.back();
    } else if press.is(TENXER_HISTORY_FORWARD) {
        browser.forward();
    } else if press.is_any(&[TENXER_HIDDEN_FILES, HIDDEN_FILES, HIDDEN_FILES_PERIOD]) {
        browser.toggle_hidden();
    } else if press.is_any(&[TENXER_THUMBNAILS_LARGER, TENXER_THUMBNAILS_LARGER_EQUAL]) {
        tab.resize_icons(1);
    } else if press.is(TENXER_THUMBNAILS_SMALLER) {
        tab.resize_icons(-1);
    } else if press.is(VISUAL_SELECT) {
        if anchor.is_some() {
            state.anchor.set(None);
        } else {
            let start = tab.cursor().unwrap_or(0);
            state.anchor.set(Some(start));
            tab.move_cursor(i64::from(start), Some(start));
            feedback(tab, "Visual");
        }
    } else if press.is(VISUAL_UNSET) {
        state.anchor.set(None);
        gtk::prelude::SelectionModelExt::unselect_all(&tab.active_model().selection);
    } else if press.is(INVERT_SELECTION) {
        let model = tab.active_model();
        let all = gtk::Bitset::new_range(0, model.len());
        let inverted = all.copy();
        inverted.subtract(&gtk::prelude::SelectionModelExt::selection(
            &model.selection,
        ));
        gtk::prelude::SelectionModelExt::set_selection(&model.selection, &inverted, &all);
    } else if press.is_any(&[FIND, FIND_BACKWARD]) {
        let kind = if press.is(FIND) {
            Prompt::Find
        } else {
            Prompt::FindBackward
        };
        tab.footer.open_prompt(kind);
    } else if press.is_any(&[REPEAT_FIND, REPEAT_FIND_REVERSED]) {
        match state.last_find.borrow().clone() {
            None => feedback(tab, "No previous find"),
            Some((text, backward)) => {
                if !tab.find(&text, backward != press.is(REPEAT_FIND_REVERSED)) {
                    report_miss(tab, &text);
                }
            }
        }
    } else if press.is(FILTER_LISTING) {
        tab.footer
            .open_prompt_with(Prompt::Filter, &tab.filter_text());
    } else if press.is(SEARCH_SUBFOLDERS) {
        if tab.location().is_some_and(|l| l.native_path().is_some()) {
            tab.footer
                .open_prompt_with(Prompt::Search, &tab.filter_text());
        } else {
            feedback(tab, "Nothing to search");
        }
    } else if press.is(CLOSE_WINDOW) {
        if let Some(window) = tab.page.root().and_downcast::<gtk::Window>() {
            gtk::prelude::GtkWindowExt::close(&window);
        }
    } else if press.is(GO) {
        tab.footer.arm_chord(Chord::Go);
    } else if press.is(SORT_BY) {
        tab.footer.arm_chord(Chord::Sort);
    } else {
        return false;
    }
    true
}

fn report_miss(tab: &Tab, query: &str) {
    tab.footer
        .show_feedback(&rust_i18n::t!("No matches for “%{query}”", query = query));
}

fn open_rename(state: &State, tab: &Rc<Tab>, selected: Vec<FileEntry>) {
    let Some(entry) = selected.last().cloned() else {
        feedback(tab, "Nothing to rename");
        return;
    };
    if entry.location.native_path().is_none() {
        feedback(tab, "Can’t rename items here");
        return;
    }
    let name = entry.display_name.clone();
    let end = match name.rfind('.') {
        Some(dot) if dot > 0 && !entry.is_directory() => name[..dot].chars().count() as i32,
        _ => -1,
    };
    if tab.footer.open_prompt_with(Prompt::Rename, &name) {
        tab.footer.select_prompt_region(0, end);
    }
    state.targets.replace(vec![entry]);
}

fn destination_prompt(state: &State, tab: &Rc<Tab>, kind: Prompt, targets: Vec<FileEntry>) {
    if targets.is_empty() {
        feedback(tab, "Nothing selected");
        return;
    }
    if tab.footer.open_prompt(kind) {
        state.targets.replace(targets);
    }
}

/// Arms **;** with the selection's numbered actions listed in the footer.
fn arm_actions(tab: &Rc<Tab>) {
    let matched = menus::numbered_actions(tab)
        .map(|(m, ..)| m)
        .unwrap_or_default();
    let mut rows: Vec<(String, String)> = if matched.is_empty() {
        vec![(
            keys::label(&Keys::Digits(ACTIONS_NUMBERS.step())),
            tr("No matching actions"),
        )]
    } else {
        matched
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, found)| (slot_key(i), found.action.name().to_owned()))
            .collect()
    };
    rows.extend(
        [
            (ACTIONS_TERMINAL, "Open terminal here"),
            (ACTIONS_COMPRESS, "Compress\u{2026}"),
            (ACTIONS_EXTRACT, "Extract here"),
            (ACTIONS_EXTRACT_TO, "Extract to\u{2026}"),
        ]
        .map(|(step, text)| (keys::label(&Keys::One(step)), tr(text))),
    );
    tab.footer.arm_chord_with(Chord::Action, rows);
}

fn archive(tab: &Tab) -> Option<FileEntry> {
    match tab.selected().as_slice() {
        [entry]
            if !entry.is_directory()
                && crate::services::ArchiveFormat::from_extension(&entry.display_name)
                    .is_some() =>
        {
            Some(entry.clone())
        }
        [] => {
            feedback(tab, "Nothing to extract");
            None
        }
        _ => {
            feedback(tab, "Select one archive");
            None
        }
    }
}

/// The second key of g or the sort chord; false when it completes none.
fn browse_chord(
    state: &State,
    tab: &Rc<Tab>,
    chord: Chord,
    press: &KeyPress,
    pins: &[Location],
) -> bool {
    let browser = &tab.browser;
    let selected = tab.selected();
    match chord {
        Chord::PreviewTop if press.is(GO_FIRST) => {
            super::preview_keys::to_top(tab);
        }
        Chord::Go | Chord::PreviewTop => {
            if chord == Chord::PreviewTop {
                tab.focus_view();
            }
            let Some(target) = go_target(press, pins) else {
                return false;
            };
            match target {
                GoTarget::FirstItem => tab.move_cursor(0, state.anchor.get()),
                GoTarget::HitFolder => match selected.last() {
                    Some(entry) if tab.filtering() => {
                        if let Some(parent) = entry.location.parent() {
                            browser.reveal_locations(parent, vec![entry.location.clone()]);
                        }
                    }
                    _ => feedback(tab, "Nothing to reveal"),
                },
                GoTarget::Prompt => {
                    tab.footer.open_prompt(Prompt::Go);
                }
                GoTarget::Pin(pin) => change_pin(tab, pin, pins),
                GoTarget::Place { location, .. } => browser.navigate(location),
                GoTarget::Folder { location, missing } => {
                    if location.native_path().is_some_and(std::path::Path::is_dir) {
                        browser.navigate(location);
                    } else {
                        tab.footer.show_feedback(&missing);
                    }
                }
                GoTarget::Missing(message) => tab.footer.show_feedback(&message),
            }
        }
        Chord::Sort => {
            let Some((key, direction)) = sort_choice(press) else {
                return false;
            };
            if let Some(depth) = browser.active_depth() {
                browser.set_sort(depth, key, direction);
            }
        }
        _ => return false,
    }
    true
}

/// The second key of the copy, actions and tabs chords.
fn window_chord(window: &Rc<Window>, tab: &Rc<Tab>, chord: Chord, press: &KeyPress) -> bool {
    let selected = tab.selected();
    match chord {
        Chord::Copy => {
            let names = if press.is(COPY_AS_PATH) {
                false
            } else if press.is(COPY_AS_NAME) {
                true
            } else {
                return false;
            };
            if selected.is_empty() {
                feedback(tab, "Nothing to copy");
            } else if names {
                let text: Vec<&str> = selected.iter().map(|e| e.display_name.as_str()).collect();
                if let Some(display) = gtk::gdk::Display::default() {
                    gtk::prelude::DisplayExt::clipboard(&display).set_text(&text.join("\n"));
                }
            } else {
                actions::copy_paths(&selected);
            }
        }
        Chord::Action => {
            if press.is(ACTIONS_TERMINAL) {
                match tab.location().filter(|l| l.native_path().is_some()) {
                    Some(here) => {
                        crate::ui::browser::desktop::launch_terminal(&here, &window.window)
                    }
                    None => feedback(tab, "Can’t open a terminal here"),
                }
            } else if press.is(ACTIONS_COMPRESS) {
                if selected.is_empty() {
                    feedback(tab, "Nothing to compress");
                } else {
                    super::operations::compress(window, tab, selected);
                }
            } else if press.is(ACTIONS_EXTRACT) {
                if let Some(entry) = archive(tab) {
                    super::operations::extract(window, tab, entry, None);
                }
            } else if press.is(ACTIONS_EXTRACT_TO) {
                if let Some(entry) = archive(tab) {
                    destination_prompt(&window.tenxer, tab, Prompt::ExtractTo, vec![entry]);
                }
            } else if let Some((digit, slot)) = action_slot(press) {
                if !menus::run_numbered_action(window, tab, slot) {
                    tab.footer
                        .show_feedback(&rust_i18n::t!("No action %{digit}", digit = digit));
                }
            } else {
                return false;
            }
        }
        Chord::Tabs => {
            if press.is(TAB_CHORD_NEW) {
                if let Some(here) = tab.location() {
                    window.open_tab(here);
                }
            } else if press.is(TAB_CHORD_CLOSE) {
                window.close_current_tab();
            } else if press.is(TAB_CHORD_PREVIOUS) {
                window.step_tab(-1);
            } else if let Some(digit) = press.digit_in(TAB_CHORD_NUMBERS) {
                window.select_tab((usize::from(digit) + 9) % 10);
            } else {
                return false;
            }
        }
        _ => return false,
    }
    true
}

fn change_pin(tab: &Tab, pin: bool, pins: &[Location]) {
    let target = match tab.selected().as_slice() {
        [entry] if entry.is_directory() => Some(entry.location.clone()),
        _ => tab.location(),
    };
    let Some(location) = target else {
        feedback(
            tab,
            if pin {
                "Nothing to pin"
            } else {
                "Nothing to unpin"
            },
        );
        return;
    };
    let name = location.display_name();
    let pinned = pins.contains(&location);
    let text = match (pin, pinned) {
        (true, true) => rust_i18n::t!("“%{name}” is already pinned", name = name),
        (false, false) => rust_i18n::t!("“%{name}” isn’t pinned", name = name),
        (true, false) => {
            super::sidebar::pin(location, name.clone());
            rust_i18n::t!("Pinned “%{name}”", name = name)
        }
        (false, true) => {
            super::sidebar::unpin(location);
            rust_i18n::t!("Unpinned “%{name}”", name = name)
        }
    };
    tab.footer.show_feedback(&text);
}

/// A typed folder: `~` is home and a relative path starts at this folder.
fn typed_folder(tab: &Tab, text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(if text == "~" {
        super::tab::home()
    } else if let Some(rest) = text.strip_prefix("~/") {
        super::tab::home().join(rest)
    } else if text.starts_with('/') {
        PathBuf::from(text)
    } else {
        tab.location()?.native_path()?.join(text)
    })
}

/// Enter in the footer prompt. Prompts that change files need `window`.
pub(super) fn submit_prompt(state: &State, window: Option<&Rc<Window>>, tab: &Rc<Tab>) {
    let footer = &tab.footer;
    let text = footer.prompt_text();
    let kind = footer.open_prompt_kind();
    footer.dismiss_prompt();
    tab.focus_view();
    let targets = state.targets.take();
    match kind {
        Some(kind @ (Prompt::Find | Prompt::FindBackward)) if !text.is_empty() => {
            let backward = kind == Prompt::FindBackward;
            state.last_find.replace(Some((text.clone(), backward)));
            if !tab.find(&text, backward) {
                report_miss(tab, &text);
            }
        }
        Some(Prompt::Filter) => tab.set_search(&text, false),
        Some(Prompt::Search) => tab.set_search(&text, true),
        Some(Prompt::Create) if !text.trim().is_empty() => {
            let Some(parent) = tab.location() else { return };
            let folder = text.trim().ends_with('/');
            let name = text.trim().trim_end_matches('/').to_string();
            tab.browser.create_exact_entry(parent, name, folder);
        }
        Some(Prompt::Rename) => {
            let name = text.trim().to_string();
            if let Some(entry) = targets.into_iter().next()
                && !name.is_empty()
                && name != entry.display_name
            {
                tab.browser.rename(entry, name);
            }
        }
        Some(kind @ (Prompt::Go | Prompt::MoveTo | Prompt::CopyTo | Prompt::ExtractTo)) => {
            let Some(path) = typed_folder(tab, &text) else {
                return;
            };
            if !path.is_dir() {
                tab.footer
                    .show_feedback(&rust_i18n::t!("No folder “%{path}”", path = path.display()));
                return;
            }
            let destination = Location::local(path);
            match kind {
                Prompt::Go => tab.browser.navigate(destination),
                Prompt::ExtractTo => {
                    if let (Some(entry), Some(window)) = (targets.into_iter().next(), window) {
                        super::operations::extract(window, tab, entry, Some(destination));
                    }
                }
                _ => {
                    if let Some(window) = window {
                        let sources = targets.into_iter().map(|e| e.location).collect();
                        actions::transfer(
                            window,
                            tab,
                            destination,
                            sources,
                            kind == Prompt::MoveTo,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}
