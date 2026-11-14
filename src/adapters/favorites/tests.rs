// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn reads_one_uri_a_line_once_each() {
    let contents = b"file:///fixture/a%20b\r\n\
        file:///fixture/report.pdf\n\
        \n\
        file:///fixture/a%20b\n\
        \xffnot-utf8\n\
        smb://server/share/notes.txt";
    assert_eq!(
        parse(contents),
        [
            Location::local("/fixture/a b"),
            Location::local("/fixture/report.pdf"),
            Location::uri("smb://server/share/notes.txt"),
        ]
    );
}

#[test]
fn what_is_written_reads_back_the_same() {
    let list = vec![
        Location::local("/fixture/with space/é.txt"),
        Location::local("/fixture/folder"),
        Location::uri("sftp://host/home/me/plan.md"),
    ];
    assert_eq!(parse(serialize(&list).as_bytes()), list);
}

#[test]
fn adding_keeps_the_order_and_skips_ones_already_there() {
    let a = Location::local("/fixture/a");
    let b = Location::local("/fixture/b");
    let c = Location::local("/fixture/c");
    let list = with_added(&[a.clone(), b.clone()], &[b.clone(), c.clone(), c.clone()]);
    assert_eq!(list, [a, b, c]);
}

#[test]
fn removing_leaves_the_others() {
    let a = Location::local("/fixture/a");
    let b = Location::local("/fixture/b");
    assert_eq!(without(&[a.clone(), b.clone()], &[b, Location::local("/x")]), [a]);
}

#[test]
fn an_edit_that_changes_nothing_reports_nothing() {
    let a = Location::local("/fixture/a");
    let list = [a.clone()];
    assert_eq!(changed(&list, |l| with_added(l, &[a.clone()])), None);
    assert_eq!(changed(&list, |l| without(l, &[Location::local("/x")])), None);
    assert_eq!(changed(&list, |l| without(l, &[a.clone()])), Some(Vec::new()));
}

#[test]
fn a_local_favorite_is_looked_at_only_when_its_mount_answers() {
    let path = Location::local("/fixture/report.pdf");
    assert_eq!(reach(&path, |_| true, &[]), Reach::Look("/fixture/report.pdf".into()));
    assert_eq!(reach(&path, |_| false, &[]), Reach::No);
}

#[test]
fn a_remote_favorite_counts_only_while_its_server_is_mounted() {
    let notes = Location::uri("sftp://host/home/me/notes.txt");
    let share = Location::uri("sftp://host/");
    let mounted = [gio::File::for_uri("sftp://host/")];
    assert_eq!(reach(&notes, |_| true, &mounted), Reach::Yes);
    assert_eq!(reach(&share, |_| true, &mounted), Reach::Yes);
    assert_eq!(reach(&notes, |_| true, &[]), Reach::No);
    let elsewhere = [gio::File::for_uri("sftp://other/")];
    assert_eq!(reach(&notes, |_| true, &elsewhere), Reach::No);
}
