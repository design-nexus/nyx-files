// SPDX-License-Identifier: MIT



use super::*;

fn renamed(count: usize) -> TransferReport {
    TransferReport {
        renamed: (0..count)
            .map(|index| RenamedItem {
                requested: Location::local(format!("/copy/item {index}")),
                saved_as: Location::local(format!("/copy/item {index} (1)")),
                cause: RenameCause::Alias,
            })
            .collect(),
        ..TransferReport::default()
    }
}

#[test]
fn a_name_fixed_for_its_control_characters_shows_where_they_were() {
    let item = RenamedItem {
        requested: Location::local("/stick/line\none\u{1}\u{85}.txt"),
        saved_as: Location::local("/stick/line_one__.txt"),
        cause: RenameCause::Invalid,
    };

    assert_eq!(
        renamed_line(&item),
        rust_i18n::t!(
            "“%{name}” was saved as “%{new_name}”",
            name = "line\u{fffd}one\u{fffd}\u{fffd}.txt",
            new_name = "line_one__.txt"
        )
    );
}





#[test]
fn a_stop_lists_renamed_items_by_cause_under_its_outcome() {
    let outcome = "Could not copy keys: Permission denied";
    let mut report = renamed(LISTED_ITEMS + 2);
    report.renamed[0].cause = RenameCause::Taken;

    let detail = outcome_detail(outcome.to_owned(), &report);

    assert!(detail.starts_with(&format!("{outcome}\n\n")), "{detail}");
    for heading in [
        "The destination can\u{2019}t tell these names apart",
        "This name was already taken",
    ] {
        let heading = crate::i18n::tr(heading);
        assert_eq!(detail.matches(&heading).count(), 1, "{heading}");
    }
    assert!(detail.contains(&renamed_line(&report.renamed[0])));
    assert_eq!(detail.matches('•').count(), 1 + LISTED_ITEMS);
    assert!(detail.ends_with(&crate::i18n::count("more_items", 1)));
}
