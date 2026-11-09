// SPDX-License-Identifier: MIT

use std::{
    cell::OnceCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use gtk::{glib, prelude::*};

use crate::{
    adapters::LocalActionStore,
    assets::icons,
    model::{ActionInput, FOLDER_CONTENT_TYPE, FileEntry, InputKind, Location},
    services::{ActionHandle, ActionRegistry, InvocationSource, JobRequest},
    ui::{
        controls::{ModalTone, focus_button, message_dialog_description, message_dialog_layout},
        modal::{
            ModalHost, dismiss_modal_layer, dismiss_modal_layer_then, modal_layer,
            show_error_dialog, show_error_dialog_after_close,
        },
    },
};

thread_local! {
    static SHARED_REGISTRY: OnceCell<Rc<ActionRegistry>> = const { OnceCell::new() };
}

pub(crate) fn shared() -> Rc<ActionRegistry> {
    let registry = SHARED_REGISTRY.with(|shared| {
        shared
            .get_or_init(|| ActionRegistry::new(LocalActionStore::new()))
            .clone()
    });
    if !registry.reload_waiting() {
        glib::MainContext::default().spawn_local(registry.reload());
    }
    registry
}

/// Manifest slugs paired with bundled Lucide assets.
pub(crate) const ACTION_ICON_CHOICES: &[(&str, &str)] = &[
    ("play", icons::PLAY),
    ("terminal", icons::TERMINAL),
    ("image", icons::PICTURES),
    ("file-code", icons::FILE_CODE),
    ("code-xml", icons::CODE_XML),
    ("file-archive", icons::FILE_ARCHIVE),
    ("file-text", icons::DOCUMENTS),
    ("copy", icons::COPY),
    ("scissors", icons::SCISSORS),
    ("pencil", icons::PENCIL),
    ("refresh", icons::REFRESH),
    ("download", icons::DOWNLOADS),
    ("scale", icons::SCALE),
    ("bug", icons::BUG),
    ("key", icons::KEY),
    ("lock", icons::LOCK),
    ("printer", icons::PRINTER),
    ("search", icons::SEARCH),
    ("folder", icons::FOLDER),
    ("check", icons::CHECK),
    ("triangle-alert", icons::TRIANGLE_ALERT),
    ("globe", icons::GLOBE),
    ("plus", icons::PLUS),
    ("monitor", icons::MONITOR),
];

pub(crate) const DEFAULT_ACTION_ICON: &str = icons::PLAY;

pub(crate) fn action_icon(icon: Option<&str>) -> &'static str {
    icon.and_then(|slug| {
        ACTION_ICON_CHOICES
            .iter()
            .find(|(candidate, _)| *candidate == slug)
            .map(|(_, asset)| *asset)
    })
    .unwrap_or(DEFAULT_ACTION_ICON)
}

pub(crate) fn is_known_action_icon(slug: &str) -> bool {
    ACTION_ICON_CHOICES
        .iter()
        .any(|(candidate, _)| *candidate == slug)
}

pub(crate) fn inputs_for_entries(entries: &[FileEntry]) -> Option<Vec<ActionInput>> {
    if entries.is_empty() {
        return None;
    }
    entries.iter().map(entry_input).collect()
}

fn entry_input(entry: &FileEntry) -> Option<ActionInput> {
    entry.location.native_path()?;
    let kind = if entry.is_directory() {
        InputKind::Folder
    } else {
        InputKind::File
    };
    Some(ActionInput {
        kind,
        name: entry.native_name.clone(),
        content_type: content_type_for(kind, &entry.native_name),
    })
}

pub(crate) fn folder_input(location: &Location) -> Option<ActionInput> {
    location.native_path()?;
    Some(ActionInput::folder(location.file_name()?))
}

fn content_type_for(kind: InputKind, name: &std::ffi::OsStr) -> Option<String> {
    if kind == InputKind::Folder {
        return Some(FOLDER_CONTENT_TYPE.to_owned());
    }
    // Menu matching must not read file contents.
    let guessed_name = name.to_string_lossy();
    let (guessed, _) = crate::services::guess_content_type(Path::new(guessed_name.as_ref()));
    (!guessed.is_empty()).then_some(guessed)
}

pub(crate) fn native_paths(entries: &[FileEntry]) -> Option<Vec<PathBuf>> {
    entries
        .iter()
        .map(|entry| entry.location.native_path().map(PathBuf::from))
        .collect()
}

pub(crate) fn run_action(
    anchor: &impl IsA<gtk::Widget>,
    action: Rc<ActionHandle>,
    paths: Vec<PathBuf>,
    parent: PathBuf,
    source: InvocationSource,
    closed: Option<Rc<dyn Fn()>>,
) {
    let current = shared().definition(action.id().to_owned());
    let anchor = anchor.as_ref().clone();
    glib::MainContext::default().spawn_local(async move {
        let refused = match current.await {
            Ok(definition) if definition == action.definition => None,
            Ok(_) => Some(crate::i18n::tr("Actions changed")),
            Err(error) => Some(error.user_message()),
        };
        if let Some(reason) = refused {
            show_error_dialog_after_close(
                &anchor,
                &crate::i18n::tr("Unable to run action"),
                &reason,
                closed.unwrap_or_else(|| Rc::new(|| {})),
            );
            return;
        }
        if action.definition.run.confirm {
            confirm_and_run(&anchor, action, paths, parent, source, closed);
            return;
        }
        enqueue(
            &anchor,
            &super::jobs::shared(),
            action,
            paths,
            parent,
            source,
        );
    });
}

fn enqueue(
    anchor: &impl IsA<gtk::Widget>,
    jobs: &Rc<crate::services::JobService>,
    action: Rc<ActionHandle>,
    paths: Vec<PathBuf>,
    parent: PathBuf,
    source: InvocationSource,
) {
    match jobs.enqueue(JobRequest {
        action,
        inputs: paths,
        parent,
        source,
    }) {
        Ok(id) => super::jobs::present_for(anchor, id),
        Err(error) => show_error_dialog(
            anchor,
            &crate::i18n::tr("Unable to run action"),
            &error.to_string(),
        ),
    }
}

fn confirm_and_run(
    anchor: &impl IsA<gtk::Widget>,
    action: Rc<ActionHandle>,
    paths: Vec<PathBuf>,
    parent: PathBuf,
    source: InvocationSource,
    closed: Option<Rc<dyn Fn()>>,
) {
    let jobs = super::jobs::shared();
    let runs = rust_i18n::t!(
        "This runs “%{value1}” on %{value2}.",
        value1 = action.name(),
        value2 = item_count_label(paths.len())
    );
    let description = action.definition.description.clone().unwrap_or_default();
    let error_anchor = anchor.as_ref().clone();
    let question = crate::i18n::tr("Run this action?");
    let name = action.name().to_owned();
    super::modal::confirm(
        anchor,
        &question,
        &[&name, &runs, &description],
        &crate::i18n::tr("Run"),
        true,
        move || {
            enqueue(
                &error_anchor,
                &jobs,
                action.clone(),
                paths.clone(),
                parent.clone(),
                source,
            );
        },
        closed,
    );
}

fn item_count_label(count: usize) -> String {
    crate::i18n::count("selected_items", count)
}
