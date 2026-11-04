// SPDX-License-Identifier: MIT

use super::*;
use crate::adapters::ActiveOperation;

fn released(root: std::path::PathBuf) -> ActiveOperation {
    let operation = ActiveOperation::begin([]);
    glib::MainContext::default().block_on(operation.release(vec![root], &gio::Cancellable::new()));
    operation
}

