// SPDX-License-Identifier: MIT

use super::*;

fn row_label(row: &gtk::Button) -> gtk::Label {
    row.child()
        .and_then(|content| content.last_child())
        .and_downcast::<gtk::Label>()
        .expect("displayed device label")
}

