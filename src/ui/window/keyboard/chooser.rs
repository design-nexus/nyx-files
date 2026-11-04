// SPDX-License-Identifier: MIT

use crate::ui::keys::{KeyPress, bindings::*};

pub(crate) const UNAVAILABLE: &str = "Not available in the file chooser";
const SINGLE: &str = "Only one item can be chosen";

pub(crate) fn refusal(
    press: &KeyPress,
    multiple: bool,
    listing_focused: bool,
) -> Option<&'static str> {
    if press.is(CLOSE_WINDOW) {
        return Some(UNAVAILABLE);
    }
    if !listing_focused {
        return None;
    }
    if press.is_any(&[
        YANK,
        PASTE_KEEP_BOTH,
        PEEK,
        YANK_CUT,
        CUT,
        COPY,
        PASTE,
        ACTIONS,
        UNYANK,
        UNYANK_X,
        PASTE_REPLACE,
        OPEN_WITH,
        MOVE_TO,
        COPY_TO,
        RESTORE,
    ]) {
        return Some(UNAVAILABLE);
    }
    let fills = press.is_any(&[
        SPACE,
        VISUAL_SELECT,
        VISUAL_UNSET,
        SELECT_ALL,
        INVERT_SELECTION,
    ]);
    (fills && !multiple).then_some(SINGLE)
}


