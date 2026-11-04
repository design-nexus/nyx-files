// SPDX-License-Identifier: MIT

use gtk::prelude::*;

use crate::{
    model::Location,
    ui::{
        keys::{
            KeyPress,
            bindings::{
                TENXER_THUMBNAILS_LARGER, TENXER_THUMBNAILS_LARGER_EQUAL,
                TENXER_THUMBNAILS_SMALLER, THUMBNAILS_LARGER, THUMBNAILS_LARGER_EQUAL,
                THUMBNAILS_RESET, THUMBNAILS_SMALLER,
            },
        },
        preferences::PreferenceManager,
    },
};

use super::DEFAULT_ICONS_THUMBNAIL_SIZE;

#[cfg(test)]
mod tests;

/// The sizes the thumbnail shortcuts and the wheel move through, finer where
/// one step is a larger share of the size.
const STEPS: [i32; 17] = [
    32, 36, 40, 44, 48, 56, 64, 72, 80, 96, 112, 128, 144, 160, 192, 224, 256,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThumbnailResize {
    /// Positive steps enlarge.
    Step(i32),
    Reset,
}

impl ThumbnailResize {
    pub(crate) fn for_shortcut(press: &KeyPress) -> Option<Self> {
        if press.is_any(&[THUMBNAILS_LARGER, THUMBNAILS_LARGER_EQUAL]) {
            Some(Self::Step(1))
        } else if press.is(THUMBNAILS_SMALLER) {
            Some(Self::Step(-1))
        } else if press.is(THUMBNAILS_RESET) {
            Some(Self::Reset)
        } else {
            None
        }
    }


}

/// `size` moved `steps` along [`STEPS`]. From a size between two steps, the
/// first step lands on the nearer one in the direction moved.
pub(crate) fn stepped(size: i32, steps: i32) -> i32 {
    let index = match steps.signum() {
        0 => return size,
        1 => STEPS.partition_point(|&step| step <= size) as i32 - 1 + steps,
        _ => STEPS.partition_point(|&step| step < size) as i32 + steps,
    };
    STEPS[index.clamp(0, STEPS.len() as i32 - 1) as usize]
}
