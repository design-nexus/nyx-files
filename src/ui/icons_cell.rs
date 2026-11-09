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

