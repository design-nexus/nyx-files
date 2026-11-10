// SPDX-License-Identifier: MIT


use crate::{
    model::{SortDirection, SortKey},
    ui::keys::{KeyPress, bindings::*},
};




pub(crate) fn sort_choice(press: &KeyPress) -> Option<(SortKey, SortDirection)> {
    use SortDirection::{Ascending, Descending};
    [
        (SORT_BY_NAME, (SortKey::Name, Ascending)),
        (SORT_BY_NAME_REVERSED, (SortKey::Name, Descending)),
        (SORT_BY_MODIFIED, (SortKey::Modified, Ascending)),
        (SORT_BY_MODIFIED_REVERSED, (SortKey::Modified, Descending)),
        (SORT_BY_SIZE, (SortKey::Size, Ascending)),
        (SORT_BY_SIZE_REVERSED, (SortKey::Size, Descending)),
        (SORT_BY_TYPE, (SortKey::Type, Ascending)),
        (SORT_BY_TYPE_REVERSED, (SortKey::Type, Descending)),
    ]
    .into_iter()
    .find(|(binding, _)| press.is(*binding))
    .map(|(_, choice)| choice)
}

/// 0 is slot 10.
pub(crate) fn action_slot(press: &KeyPress) -> Option<(u8, usize)> {
    let digit = press.digit_in(ACTIONS_NUMBERS)?;
    let slot = if digit == 0 { 10 } else { usize::from(digit) };
    Some((digit, slot))
}

pub(crate) fn slot_key(index: usize) -> String {
    use crate::ui::keys::{self, bindings::ACTIONS_NUMBERS};
    keys::digit_label(ACTIONS_NUMBERS.step(), ((index + 1) % 10) as u8)
}


