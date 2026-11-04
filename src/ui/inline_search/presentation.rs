// SPDX-License-Identifier: MIT

use super::collection::{NameHighlights, ResultKind};
use crate::{
    model::Location,
    services::SearchItem,
    ui::{accessibility, browser, collection_edit::EditWidgets, icons_cell, thumbnail},
};
use gtk::prelude::*;
use std::path::Path;

#[derive(Clone)]
enum Labels {
    Rows {
        name: gtk::Label,
        origin: gtk::Label,
    },
    Icons {
        name: gtk::Inscription,
        origin: gtk::Label,
    },
}

/// The factory owns its structure; behavior receives explicit editor and display handles.
#[derive(Clone)]
pub(super) struct ResultWidgets {
    pub(super) widget: gtk::Box,
    pub(super) edit: EditWidgets,
    icon: thumbnail::ThumbnailSlot,
    labels: Labels,
}

impl ResultWidgets {


}

