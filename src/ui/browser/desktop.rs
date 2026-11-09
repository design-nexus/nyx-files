// SPDX-License-Identifier: MIT

use crate::adapters::gio_file_for_location;
use crate::app::Browser;
use crate::model::{FileEntry, Location};
use crate::ui::browser::is_trash_location;
use crate::ui::controls::{
    ModalTone, message_dialog_description, };
use crate::ui::modal::{ModalHost, dismiss_modal_layer, modal_layer, show_error_dialog};
use crate::ui::terminal;
use gtk::gio;
use gtk::prelude::*;
use std::path::Path;
use std::process::{Command, Stdio};
use std::rc::{Rc, Weak};

#[cfg(test)]
mod tests;


pub(crate) fn open_location_at(
    location: &Location,
    position: Option<std::time::Duration>,
    parent: &impl IsA<gtk::Widget>,
    browser: &Rc<Browser>,
) {
    if is_trash_location(location) {
        show_error_dialog(
            parent,
            &crate::i18n::tr("Unable to open item"),
            &crate::i18n::tr("Items in Trash cannot be opened"),
        );
        return;
    }
    let file = gio_file_for_location(location);
    let context = parent.as_ref().display().app_launch_context();
    let parent = parent.as_ref().downgrade();
    let location = location.clone();
    let browser = Rc::downgrade(browser);
    glib::MainContext::default().spawn_local(async move {
        match resolve_default_application(&file).await {
            Ok(DefaultApplication {
                content_type,
                app: Some(app),
                ..
            }) => {
                let result = crate::ui::open_with::launch_at(
                    &app,
                    &file,
                    &content_type,
                    position,
                    Some(&context),
                );
                if let Some(parent) = parent.upgrade() {
                    report_open_result(&location, &parent, result);
                }
            }
            Ok(DefaultApplication {
                content_type,
                app: None,
                executable,
            }) => {
                let Some(parent) = parent.upgrade() else {
                    return;
                };
                if executable && file.is_native() && location.native_path().is_some() {
                    confirm_run_program(&location, &parent);
                } else {
                    show_open_with_fallback(&parent, file, &content_type, browser).await;
                }
            }
            Err(error) => {
                if let Some(parent) = parent.upgrade() {
                    report_open_result(&location, &parent, Err(error));
                }
            }
        }
    });
}

struct DefaultApplication {
    content_type: String,
    app: Option<gio::AppInfo>,
    executable: bool,
}

async fn resolve_default_application(file: &gio::File) -> Result<DefaultApplication, glib::Error> {
    let info = file
        .query_info_future(
            "standard::type,standard::content-type,unix::mode",
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await?;
    if info.file_type() == gio::FileType::SymbolicLink {
        return Err(glib::Error::new(
            gio::IOErrorEnum::Failed,
            &crate::i18n::tr("Broken symbolic links cannot be opened with an application"),
        ));
    }
    let content_type = info
        .content_type()
        .map(|value| value.to_string())
        .ok_or_else(|| {
            glib::Error::new(
                gio::IOErrorEnum::Failed,
                &crate::i18n::tr("Unable to determine the selected file type"),
            )
        })?;
    let executable = info.file_type() == gio::FileType::Regular
        && info.attribute_uint32(gio::FILE_ATTRIBUTE_UNIX_MODE) & 0o111 != 0;
    let app = if crate::ui::open_with::uses_portal() {
        None
    } else {
        let requires_uris = crate::ui::open_with::requires_uri_handlers(std::slice::from_ref(file));
        gio::AppInfo::default_for_type(&content_type, requires_uris)
    };
    Ok(DefaultApplication {
        content_type,
        app,
        executable,
    })
}

async fn show_open_with_fallback(
    parent: &impl IsA<gtk::Widget>,
    file: gio::File,
    _content_type: &str,
    browser: Weak<Browser>,
) {
    let Some(window) = parent
        .as_ref()
        .root()
        .and_downcast::<gtk::Window>()
        .filter(|window| {
            window.is_visible() && !crate::ui::window::foreign_modal_visible(window, None)
        })
    else {
        return;
    };
    if crate::ui::open_with::uses_portal() {
        crate::ui::open_with::launch_with_portal(&window, vec![file], false);
        return;
    }
    let files = vec![file];
    let Some(Ok(apps)) = crate::ui::open_with::resolve(&files, || true).await else {
        return;
    };
    let overlay = crate::ui::modal::window_overlay(&window);
    crate::ui::open_with::dialog(&window, overlay, files, apps, true);
    if let Some(browser) = browser.upgrade() {
        browser.focus_active();
    }
}

fn report_open_result(
    location: &Location,
    parent: &impl IsA<gtk::Widget>,
    result: Result<(), glib::Error>,
) {
    if let Err(error) = result {
        tracing::warn!(
            backend = %location.backend_name(),
            error_domain = ?error.domain(),
            error_code = error.code(),
            "unable to open file"
        );
        tracing::debug!(
            location = %location.diagnostic_path(),
            "file open location"
        );
        let detail = if error.matches(gio::IOErrorEnum::NotSupported)
            && gio_file_for_location(location).is_native()
        {
            crate::i18n::tr("No application is registered for this file")
        } else {
            error.message().to_owned()
        };
        show_error_dialog(parent, &crate::i18n::tr("Unable to open file"), &detail);
    }
}

pub(crate) fn entry_is_regular_executable(entry: &FileEntry) -> bool {
    entry.location.native_path().is_some()
        && matches!(
            entry.kind,
            crate::model::EntryKind::File | crate::model::EntryKind::FileSymbolicLink
        )
        && matches!(entry.mode, crate::model::MetadataValue::Known(mode) if mode & 0o111 != 0)
}

pub(crate) fn confirm_run_program(location: &Location, parent: &impl IsA<gtk::Widget>) {
    let name = location.display_name();
    let detail = rust_i18n::t!(
        "“%{name}” is an executable file. Only run programs you trust.",
        name = name
    );
    let (run_location, error_parent) = (location.clone(), parent.as_ref().clone());
    crate::ui::modal::confirm(
        parent,
        &crate::i18n::tr("Run this program?"),
        &[&detail],
        &crate::i18n::tr("Run"),
        true,
        move || run_program(&run_location, &error_parent),
        None,
    );
}

fn run_program(location: &Location, parent: &impl IsA<gtk::Widget>) {
    let Some(path) = location.native_path().map(Path::to_path_buf) else {
        return;
    };
    let parent = parent.as_ref().downgrade();
    let launched = terminal::spawn_reaped(move || {
        program_command(&path, terminal::Terminal::resolve)
            .and_then(|mut command| command.spawn())
            .map_err(|error| {
                tracing::warn!(%error, "unable to run program");
                error.to_string()
            })
    });
    glib::MainContext::default().spawn_local(async move {
        if let (Err(detail), Some(parent)) = (launched.await, parent.upgrade()) {
            show_error_dialog(&parent, &crate::i18n::tr("Unable to run program"), &detail);
        }
    });
}

fn program_command(
    path: &Path,
    resolve_terminal: impl FnOnce() -> Option<terminal::Terminal>,
) -> std::io::Result<Command> {
    if path.extension().is_some_and(|extension| extension == "sh") {
        let terminal = resolve_terminal().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                terminal::no_terminal_message(),
            )
        })?;
        // Pass the path as data, preserving the script's shebang and avoiding shell injection.
        let mut command = terminal.exec_command(&[
            std::ffi::OsStr::new("/bin/sh"),
            std::ffi::OsStr::new("-c"),
            std::ffi::OsStr::new(
                "cd -- \"$2\" && \"$1\"; status=$?; printf '\\n'; printf \"$3\" \"$status\"; IFS= read -r answer; exit \"$status\"",
            ),
            std::ffi::OsStr::new("strata-run"),
            path.as_os_str(),
            path.parent().unwrap_or_else(|| Path::new(".")).as_os_str(),
            std::ffi::OsStr::new(&exit_prompt_format()),
        ]);
        command.current_dir(path.parent().unwrap_or_else(|| Path::new(".")));
        return Ok(command);
    }
    let mut command = Command::new(path);
    command
        .current_dir(path.parent().unwrap_or_else(|| Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

/// A printf format whose only conversion is the exit status.
fn exit_prompt_format() -> String {
    rust_i18n::t!(
        "Process exited with status %{status}. Press Enter to close…",
        status = "\u{1}"
    )
    .replace('%', "%%")
    .replace('\u{1}', "%s")
}



pub(crate) fn launch_terminal(location: &Location, parent: &impl IsA<gtk::Widget>) {
    let Some(path) = location.native_path() else {
        show_error_dialog(
            parent,
            &crate::i18n::tr("Unable to open terminal"),
            &crate::i18n::tr("This location is not a local folder"),
        );
        return;
    };
    if is_trash_location(location) {
        show_error_dialog(
            parent,
            &crate::i18n::tr("Unable to open terminal"),
            &crate::i18n::tr("Terminal cannot be opened in Trash"),
        );
        return;
    }
    let path = path.to_path_buf();
    tracing::debug!(
        location = %location.diagnostic_path(),
        "opening terminal"
    );
    let parent = parent.as_ref().downgrade();
    let launched = terminal::spawn_reaped(move || {
        let Some(terminal) = terminal::Terminal::resolve() else {
            tracing::warn!("no terminal emulator found on PATH");
            return Err(terminal::no_terminal_message());
        };
        terminal.directory_command(&path).spawn().map_err(|error| {
            let program = terminal.program().to_string_lossy();
            tracing::warn!(%error, %program, "unable to launch terminal");
            terminal.launch_failure(&error)
        })
    });
    glib::MainContext::default().spawn_local(async move {
        if let (Err(detail), Some(parent)) = (launched.await, parent.upgrade()) {
            show_error_dialog(
                &parent,
                &crate::i18n::tr("Unable to open terminal"),
                &detail,
            );
        }
    });
}
