// SPDX-License-Identifier: MIT
//! Cloud accounts use the existing local navigation/operation stack through FUSE.
use super::*;


/// Cloud mounts each account as an rclone FUSE folder directly in the home directory.
pub(crate) fn is_cloud_mount(path: &std::path::Path) -> bool {
    if path.parent() != Some(super::home_directory().as_path()) {
        return false;
    }
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\134")
        .replace(' ', "\\040")
        .replace('\t', "\\011")
        .replace('\n', "\\012");
    std::fs::read_to_string("/proc/self/mountinfo")
        .unwrap_or_default()
        .lines()
        .any(|line| {
            let mut halves = line.splitn(2, " - ");
            let mount_point = halves.next().and_then(|f| f.split_whitespace().nth(4));
            let fs_type = halves.next().and_then(|f| f.split_whitespace().next());
            mount_point == Some(escaped.as_str()) && fs_type == Some("fuse.rclone")
        })
}
