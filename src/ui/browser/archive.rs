// SPDX-License-Identifier: MIT

//! Compress and extract dialogs for the browser view.
//!
//! These methods live on [`ViewState`] and start work through the shared
//! [`crate::app::Browser`] controller; they must not create a separate archive
//! pipeline. Password-capable extracts record a retry so a later failure can
//! reopen [`ViewState::show_extract_password_dialog`].
//!
//! # Entry points
//!
//! - [`ViewState::show_compress_dialog`]
//! - [`ViewState::extract_entry`]
//! - [`ViewState::show_extract_to_dialog`]
//! - [`ViewState::show_extract_password_dialog`]

use crate::ui::browser::ViewState;




impl ViewState {







}


