// SPDX-License-Identifier: MIT

use crate::adapters::directory_summary::DirectorySummary;
use gtk::prelude::*;

pub(crate) mod media;

pub fn format_permissions(mode: u32) -> String {
    let kind = if mode & 0o170000 == 0o040000 {
        'd'
    } else {
        '-'
    };
    let mut symbolic = String::with_capacity(10);
    symbolic.push(kind);
    for (mask, character) in [
        (0o400, 'r'),
        (0o200, 'w'),
        (0o100, 'x'),
        (0o040, 'r'),
        (0o020, 'w'),
        (0o010, 'x'),
        (0o004, 'r'),
        (0o002, 'w'),
        (0o001, 'x'),
    ] {
        symbolic.push(if mode & mask != 0 { character } else { '-' });
    }
    format!("{symbolic}  {:03o}", mode & 0o777)
}

pub(crate) fn directory_counts_label(summary: &DirectorySummary) -> String {
    let prefix = if summary.truncated() { "≥ " } else { "" };
    rust_i18n::t!(
        "%{files}, %{folders}",
        files = format!(
            "{prefix}{}",
            crate::i18n::count("files", summary.visible_file_count)
        ),
        folders = format!(
            "{prefix}{}",
            crate::i18n::count("folders", summary.visible_folder_count)
        ),
    )
    .into_owned()
}

pub(crate) fn measurement_warning_text(summary: &DirectorySummary) -> Option<String> {
    let mut reasons = Vec::new();
    if summary.issues.unreadable {
        reasons.push(crate::i18n::tr("Some folders or entries couldn't be read."));
    }
    if summary.issues.timed_out {
        reasons.push(crate::i18n::tr(
            "The five-minute calculation limit was reached.",
        ));
    }
    if summary.issues.depth_limited {
        reasons.push(crate::i18n::tr(
            "Some folders exceeded the 64-level nesting limit.",
        ));
    }
    (!reasons.is_empty()).then(|| {
        rust_i18n::t!(
            "Totals are incomplete.\n%{value1}",
            value1 = reasons.join("\n")
        )
        .into_owned()
    })
}

