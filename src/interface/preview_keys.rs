// SPDX-License-Identifier: MIT

//! Keys while the preview drawer has focus: documents scroll, archives step
//! through their entries, players take media keys. Moving the listing behind
//! the drawer needs the keys handed back first.

use gtk::{gdk::ModifierType as Modifiers, prelude::*};

use crate::ui::{
    keys::{Binding, KeyPress, SHORTCUT_MODIFIERS, bindings::*},
    preview::{ArchiveMove, DocumentScroll, PreviewSurface, media_key_command},
    tenxer_mode::Chord,
};

use super::tab::Tab;

const CONTROL: Modifiers = Modifiers::CONTROL_MASK;
const SHIFT: Modifiers = Modifiers::SHIFT_MASK;

/// GTK's own text selection keys, which a focused document keeps.
const DOCUMENT_SELECTION: [Binding; 8] = [
    EXTEND_UP.ignoring(CONTROL),
    EXTEND_DOWN.ignoring(CONTROL),
    EXTEND_LEFT.ignoring(CONTROL),
    EXTEND_RIGHT.ignoring(CONTROL),
    EXTEND_TO_FIRST.ignoring(CONTROL),
    EXTEND_TO_LAST.ignoring(CONTROL),
    DOCUMENT_START,
    DOCUMENT_END,
];

/// What a key in the drawer did.
pub(super) enum Outcome {
    /// Handled here.
    Stop,
    /// The focused widget in the drawer keeps it.
    Widget,
    /// The listing's handlers take it (focus is back on the listing).
    Listing,
}

fn focused_in_preview(tab: &Tab) -> Option<gtk::Widget> {
    let root = tab.page.root()?;
    let focused = root.focus()?;
    tab.preview.owns_focus(Some(&focused)).then_some(focused)
}

pub(super) fn has_focus(tab: &Tab) -> bool {
    focused_in_preview(tab).is_some()
}

fn back_to_listing(tab: &Tab) {
    tab.focus_view();
}

/// Every map: Esc closes the drawer, Shift+Tab hands keys back, Ctrl+0 resets
/// PDF zoom. In 10xer mode the drawer's motion keys follow too.
pub(super) fn handle(tab: &Tab, press: &KeyPress, tenxer: bool) -> Outcome {
    let Some(focused) = focused_in_preview(tab) else {
        return Outcome::Listing;
    };
    if press.is(ESCAPE) {
        if !tab.preview.close_find_if_open() {
            tab.preview.close();
            back_to_listing(tab);
        }
        return Outcome::Stop;
    }
    if press.is(FOCUS_PREVIOUS.ignoring(CONTROL | Modifiers::ALT_MASK | Modifiers::SUPER_MASK)) {
        back_to_listing(tab);
        return Outcome::Stop;
    }
    if tab.preview.reset_pdf_zoom(press) {
        return Outcome::Stop;
    }
    let surface = tab.preview.surface(&focused);
    if surface != PreviewSurface::Text
        && crate::ui::preview::PreviewDrawer::is_find_key(press)
        && tab.preview.open_find()
    {
        return Outcome::Stop;
    }
    if !tenxer {
        if let Some(command) = media_key_command(press)
            && surface == PreviewSurface::Media
        {
            tab.preview.media_key(command);
            return Outcome::Stop;
        }
        return Outcome::Widget;
    }
    if surface == PreviewSurface::Text
        && !press.is_any(&[REFERENCE.ignoring(SHORTCUT_MODIFIERS), TENXER_MODE])
    {
        return Outcome::Widget;
    }
    if press.is(PEEK) {
        tab.preview.close();
        back_to_listing(tab);
        return Outcome::Stop;
    }
    if surface == PreviewSurface::Document && press.is_any(&DOCUMENT_SELECTION) {
        return Outcome::Widget;
    }
    if press.is_any(&[SCROLL_PREVIEW_DOWN, SCROLL_PREVIEW_UP]) {
        if surface != PreviewSurface::Media {
            let lines = if press.is(SCROLL_PREVIEW_DOWN) { 1 } else { -1 };
            tab.preview.scroll_document(DocumentScroll::Line(lines));
        }
        return Outcome::Stop;
    }
    if press.is(FOCUS_NEXT) {
        return Outcome::Widget;
    }
    if press.is(GO) && matches!(surface, PreviewSurface::Document | PreviewSurface::Archive) {
        tab.footer.arm_chord(Chord::PreviewTop);
        return Outcome::Stop;
    }
    let handled = match surface {
        PreviewSurface::Document => document_key(tab, press),
        PreviewSurface::Archive => archive_key(tab, press),
        PreviewSurface::Media => media_key(tab, press),
        PreviewSurface::Control | PreviewSurface::Text => {
            let listing = surface == PreviewSurface::Control
                && !press.is(ENTER_ANY)
                && reaches_listing(press);
            if !listing && !press.modifiers().intersects(CONTROL | Modifiers::ALT_MASK) {
                return Outcome::Widget;
            }
            false
        }
    };
    if handled {
        return Outcome::Stop;
    }
    if passes_through(press) {
        return Outcome::Listing;
    }
    if press.is_any(&[COPY, SELECT_ALL]) && surface != PreviewSurface::Media {
        return Outcome::Widget;
    }
    if reaches_listing(press) {
        back_to_listing(tab);
        return Outcome::Listing;
    }
    Outcome::Stop
}

/// **g g** in a focused document or archive preview.
pub(super) fn to_top(tab: &Tab) -> bool {
    let Some(focused) = focused_in_preview(tab) else {
        return false;
    };
    match tab.preview.surface(&focused) {
        PreviewSurface::Document => {
            tab.preview.scroll_document(DocumentScroll::Start);
        }
        PreviewSurface::Archive => {
            tab.preview.archive_edge(false);
        }
        _ => return false,
    }
    true
}

fn document_key(tab: &Tab, press: &KeyPress) -> bool {
    if press.is_any(&[VIM_LEFT, LEFT]) {
        back_to_listing(tab);
        return true;
    }
    let Some((_, motion)) = [
        (HALF_PAGE_UP, DocumentScroll::HalfPage(-1)),
        (HALF_PAGE_DOWN, DocumentScroll::HalfPage(1)),
        (TENXER_PAGE_UP, DocumentScroll::Page(-1)),
        (TENXER_PAGE_UP_PAGE_UP, DocumentScroll::Page(-1)),
        (TENXER_PAGE_DOWN, DocumentScroll::Page(1)),
        (TENXER_PAGE_DOWN_PAGE_DOWN, DocumentScroll::Page(1)),
        (FIRST_ITEM_UP, DocumentScroll::Start),
        (LAST_ITEM_DOWN, DocumentScroll::End),
        (VIM_DOWN, DocumentScroll::Line(1)),
        (DOWN, DocumentScroll::Line(1)),
        (VIM_UP, DocumentScroll::Line(-1)),
        (UP, DocumentScroll::Line(-1)),
        (PAGE_UP.ignoring(SHIFT), DocumentScroll::Page(-1)),
        (PAGE_DOWN.ignoring(SHIFT), DocumentScroll::Page(1)),
        (FIRST_ITEM, DocumentScroll::Start),
        (LAST_ITEM, DocumentScroll::End),
        (TENXER_LAST_ITEM, DocumentScroll::End),
    ]
    .into_iter()
    .find(|(key, _)| press.is(*key)) else {
        return false;
    };
    tab.preview.scroll_document(motion);
    true
}

fn archive_key(tab: &Tab, press: &KeyPress) -> bool {
    if press.is_any(&[VIM_DOWN, DOWN]) {
        tab.preview.archive_key(ArchiveMove::Next);
    } else if press.is_any(&[VIM_UP, UP]) {
        tab.preview.archive_key(ArchiveMove::Previous);
    } else if press.is_any(&[VIM_RIGHT, RIGHT, ENTER]) {
        tab.preview.archive_key(ArchiveMove::Open);
    } else if press.is_any(&[VIM_LEFT, LEFT]) {
        if tab.preview.archive_at_root() {
            back_to_listing(tab);
        } else {
            tab.preview.archive_key(ArchiveMove::Parent);
        }
    } else if press.is(FIRST_ITEM) {
        tab.preview.archive_edge(false);
    } else if press.is_any(&[LAST_ITEM, TENXER_LAST_ITEM]) {
        tab.preview.archive_edge(true);
    } else {
        return false;
    }
    true
}

fn media_key(tab: &Tab, press: &KeyPress) -> bool {
    if press.is(VIM_LEFT) {
        back_to_listing(tab);
        return true;
    }
    let Some(command) = media_key_command(press) else {
        return false;
    };
    tab.preview.media_key(command);
    true
}

/// Window commands that neither read nor change the listing.
fn passes_through(press: &KeyPress) -> bool {
    press.is_any(&[
        REFERENCE,
        REFRESH,
        SEARCH,
        LOCATION,
        SETTINGS,
        HIDDEN_FILES,
        HIDDEN_FILES_PERIOD,
        TENXER_SIDEBAR,
        VIEW_COLUMNS,
        VIEW_ICONS,
        VIEW_LIST,
        TEXT_LARGER,
        TEXT_LARGER_EQUAL,
        TEXT_SMALLER,
        TEXT_RESET,
        SIDEBAR_FOCUS,
        TENXER_MODE,
        UNDO,
        REDO,
        REDO_Y,
        MEDIA_PLAY_PAUSE,
        MEDIA_SEEK_BACK,
        MEDIA_SEEK_FORWARD,
        MEDIA_VOLUME_UP,
        MEDIA_VOLUME_DOWN,
        MEDIA_MUTE,
    ])
}

fn reaches_listing(press: &KeyPress) -> bool {
    press.is_any(&[
        GO,
        JUMP_VISITED,
        FILTER_LISTING,
        SEARCH_SUBFOLDERS,
        REPEAT_FIND,
        FIND,
        BACKSPACE,
        TENXER_HISTORY_BACK,
        TENXER_HISTORY_FORWARD,
        JUMP_RECENT,
        REPEAT_FIND_REVERSED,
        FIND_BACKWARD,
        HISTORY_BACK,
        HISTORY_FORWARD,
        PARENT_FOLDER,
        OPEN,
        YANK,
        YANK_CUT,
        PASTE_KEEP_BOTH,
        TENXER_DELETE,
        CREATE,
        COPY_AS,
        TENXER_RENAME,
        RENAME,
        CONTEXT_MENU,
        UNYANK,
        UNYANK_X,
        PASTE_REPLACE,
        TENXER_DELETE_PERMANENTLY,
        OPEN_WITH,
        MOVE_TO,
        COPY_TO,
        RESTORE,
        CONTEXT_MENU_F10,
        SORT_BY,
        ACTIONS,
        TENXER_HIDDEN_FILES,
        DELETE,
        DELETE_PERMANENTLY,
        ENTER,
        PROPERTIES,
        COPY,
        CUT,
        PASTE,
        NEW_FOLDER,
    ])
}
