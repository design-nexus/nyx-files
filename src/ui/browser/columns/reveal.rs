// SPDX-License-Identifier: MIT

use super::*;

#[derive(Clone, Copy, Debug)]
pub(in crate::ui::browser) struct ColumnSpan {
    pub left: f64,
    pub right: f64,
    pub trailing: f64,
}

impl ColumnSpan {
    pub fn width(self) -> f64 {
        self.right - self.left
    }

}

impl ViewState {


}
