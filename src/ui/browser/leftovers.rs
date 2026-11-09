// SPDX-License-Identifier: MIT

use std::cell::RefCell;

use gtk::prelude::*;

use crate::{
    model::{Location, hidden_names::Leftover},
    services::LoadHandle,
};

pub(crate) fn title() -> String {
    crate::i18n::tr("Items left by an interrupted operation")
}

pub(crate) fn heading(kind: Leftover) -> String {
    crate::i18n::tr(match kind {
        Leftover::Originals => "Set-aside originals",
        Leftover::MergeOriginals => "Originals set aside by a merge",
        Leftover::Original => "Set-aside original",
        Leftover::MaybeOriginal => "Possible original",
        Leftover::IncompleteCopy | Leftover::Probe => "Incomplete copy",
    })
}

pub(crate) fn explanation(kind: Leftover) -> String {
    crate::i18n::tr(match kind {
        Leftover::Originals => {
            "Originals moved aside before being replaced. Restore puts them back."
        }
        Leftover::MergeOriginals => {
            "Originals a merge moved aside before replacing them. Restore puts them back."
        }
        Leftover::Original => {
            "An original an earlier version kept under a hidden name. Restore brings it back under a new name."
        }
        Leftover::MaybeOriginal => {
            "May be a replaced original or an incomplete copy. Restore brings it back under a new name."
        }
        Leftover::IncompleteCopy | Leftover::Probe => {
            "A copy that never finished. Its source was not changed."
        }
    })
}

pub(crate) fn restorable(kind: Leftover, folder: &Location) -> bool {
    match kind {
        Leftover::Originals | Leftover::MergeOriginals => folder.native_path().is_some(),
        Leftover::Original | Leftover::MaybeOriginal => true,
        Leftover::IncompleteCopy | Leftover::Probe => false,
    }
}

pub(crate) fn recovered_name(taken: &[String]) -> String {
    let base = crate::i18n::tr("Recovered item");
    std::iter::once(base.clone())
        .chain((1..).map(|number| format!("{base} ({number})")))
        .find(|name| !taken.contains(name))
        .unwrap_or(base)
}

