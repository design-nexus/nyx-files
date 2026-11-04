// SPDX-License-Identifier: MIT


use gtk::{gio, glib, prelude::*};








pub(crate) enum Kind {
    Directory,
    File,
}




/// A no-follow fallback preserves broken native symlinks as revealable entries.
pub(crate) async fn query_kind(
    file: &gio::File,
    operation: Option<&gio::MountOperation>,
) -> Result<Kind, glib::Error> {
    let mut mounted = false;
    loop {
        match file
            .query_info_future(
                "standard::type",
                gio::FileQueryInfoFlags::NONE,
                glib::Priority::DEFAULT,
            )
            .await
        {
            Ok(info) => {
                return Ok(match info.file_type() {
                    gio::FileType::Directory | gio::FileType::Mountable => Kind::Directory,
                    _ => Kind::File,
                });
            }
            Err(error)
                if !mounted
                    && operation.is_some()
                    && error.matches(gio::IOErrorEnum::NotMounted) =>
            {
                mounted = true;
                if let Err(error) = file
                    .mount_enclosing_volume_future(gio::MountMountFlags::NONE, operation)
                    .await
                    && !error.matches(gio::IOErrorEnum::AlreadyMounted)
                {
                    return Err(error);
                }
            }
            Err(error) => {
                return match file
                    .query_info_future(
                        "standard::is-symlink",
                        gio::FileQueryInfoFlags::NOFOLLOW_SYMLINKS,
                        glib::Priority::DEFAULT,
                    )
                    .await
                {
                    Ok(info) if info.is_symlink() => Ok(Kind::File),
                    _ => Err(error),
                };
            }
        }
    }
}










#[cfg(test)]
mod tests;
