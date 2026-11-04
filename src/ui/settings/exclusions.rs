// SPDX-License-Identifier: MIT

use std::rc::Rc;

use gtk::{gio, prelude::*};

use crate::{
    assets::icons,
    services::SearchExclusions,
    ui::{
        controls::{form_entry, form_error_label, set_form_field_error},
        preferences::PreferenceManager,
    },
};

#[cfg(test)]
mod tests;

fn escaped(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}

/// The add row, then a row per exclusion, for the Search exclusions card.
pub(crate) fn search_exclusions_control(manager: &Rc<PreferenceManager>) -> gtk::Box {
    let control = gtk::Box::new(gtk::Orientation::Vertical, 0);
    control.set_hexpand(true);
    let field = form_entry();
    field.set_width_chars(22);
    field.set_placeholder_text(Some(&crate::i18n::tr(
        "Folder name (e.g. .venv) or ~/path…",
    )));
    super::super::accessibility::set_label(&field, &crate::i18n::tr("Search exclusion"));

    let browse_btn = gtk::Button::with_label(&crate::i18n::tr("Browse…"));
    let add_btn = gtk::Button::with_label(&crate::i18n::tr("Add"));

    let input = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    input.append(&field);
    input.append(&browse_btn);
    input.append(&add_btn);

    let input_row = nyx_ui::widgets::row(
        &crate::i18n::tr("Add exclusion"),
        &escaped(&crate::i18n::tr(
            "Exclude folder names or directory paths from global search.",
        )),
        Some(input.upcast_ref()),
    );
    let error_label = form_error_label();
    if let Some(text) = input_row.first_child().and_downcast::<gtk::Box>() {
        text.append(&error_label);
    }
    control.append(&input_row);

    let exclusions_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    control.append(&exclusions_box);

    let weak_manager = Rc::downgrade(manager);
    manager.bind_preference(
        &exclusions_box,
        PreferenceManager::search_exclusions,
        move |widget, exclusions| {
            if let Some(manager) = weak_manager.upgrade()
                && let Some(container) = widget.downcast_ref::<gtk::Box>()
            {
                render_exclusion_rows(container, &manager, exclusions);
            }
        },
    );

    let field_for_add = field.downgrade();
    let error_for_add = error_label.downgrade();
    let manager_for_add = Rc::downgrade(manager);
    let do_add = Rc::new(move || {
        let (Some(field_for_add), Some(error_for_add), Some(manager_for_add)) = (
            field_for_add.upgrade(),
            error_for_add.upgrade(),
            manager_for_add.upgrade(),
        ) else {
            return;
        };
        let raw = field_for_add.text().to_string();
        let current = manager_for_add.search_exclusions();
        match validate_exclusion_input(&raw, &current) {
            Ok(trimmed) => {
                set_form_field_error(&field_for_add, &error_for_add, None);
                let mut updated = current;
                updated.push(trimmed);
                manager_for_add.set_search_exclusions(updated);
                field_for_add.set_text("");
            }
            Err(error) => {
                set_form_field_error(&field_for_add, &error_for_add, Some(error));
            }
        }
    });

    let do_add_activate = do_add.clone();
    field.connect_activate(move |_| {
        do_add_activate();
    });

    let error_for_change = error_label.downgrade();
    field.connect_changed(move |field| {
        if let Some(error) = error_for_change.upgrade() {
            set_form_field_error(field, &error, None);
        }
    });

    let do_add_click = do_add.clone();
    add_btn.connect_clicked(move |_| {
        do_add_click();
    });

    let field_for_browse = field.downgrade();
    let error_for_browse = error_label.downgrade();
    browse_btn.connect_clicked(move |button| {
        let Some(window) = button.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let dialog = gtk::FileDialog::builder()
            .title(crate::i18n::tr("Select folder to exclude"))
            .modal(true)
            .build();
        let field = field_for_browse.clone();
        let error = error_for_browse.clone();
        dialog.select_folder(Some(&window), gio::Cancellable::NONE, move |result| {
            let (Ok(file), Some(field), Some(error)) = (result, field.upgrade(), error.upgrade())
            else {
                return;
            };
            if let Some(path) = file.path() {
                let display = super::general::abbreviate_home(&path);
                set_form_field_error(&field, &error, None);
                field.set_text(&display);
                field.set_position(-1);
                field.grab_focus();
            }
        });
    });

    control
}

fn render_exclusion_rows(
    container: &gtk::Box,
    manager: &Rc<PreferenceManager>,
    exclusions: Vec<String>,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    if exclusions.is_empty() {
        let row = nyx_ui::widgets::row(
            "",
            &escaped(&crate::i18n::tr(
                "No custom exclusions added. Common tool caches (.venv, node_modules, target, etc.) are excluded automatically.",
            )),
            None,
        );
        if let Some(title) = row.first_child().and_then(|text| text.first_child()) {
            title.set_visible(false);
        }
        container.append(&row);
        return;
    }
    for item in exclusions {
        let is_path = SearchExclusions::is_directory_path(&item);
        let remove = nyx_ui::widgets::icon_button(
            "user-trash-symbolic",
            &crate::i18n::tr("Remove exclusion"),
        );
        super::super::accessibility::set_label(
            &remove,
            &rust_i18n::t!("Remove exclusion %{item}", item = item),
        );
        let row = nyx_ui::widgets::row(
            &item,
            &escaped(&crate::i18n::tr(if is_path {
                "Directory"
            } else {
                "Folder name"
            })),
            Some(remove.upcast_ref()),
        );
        let icon = crate::assets::primary_icon(icons::FOLDER, 16);
        icon.set_valign(gtk::Align::Center);
        row.prepend(&icon);
        let weak_manager = Rc::downgrade(manager);
        remove.connect_clicked(move |_| {
            if let Some(manager) = weak_manager.upgrade() {
                let mut exclusions = manager.search_exclusions();
                exclusions.retain(|candidate| candidate != &item);
                manager.set_search_exclusions(exclusions);
            }
        });
        container.append(&row);
    }
}

pub(super) fn validate_exclusion_input(
    raw: &str,
    current: &[String],
) -> Result<String, &'static str> {
    let normalized = SearchExclusions::normalize_entry(raw)?;
    let is_path = SearchExclusions::is_directory_path(&normalized);
    let is_duplicate = current
        .iter()
        .filter_map(|item| SearchExclusions::normalize_entry(item).ok())
        .any(|existing| {
            if is_path {
                existing == normalized
            } else {
                existing.eq_ignore_ascii_case(&normalized)
            }
        });
    if is_duplicate {
        return Err("This exclusion has already been added.");
    }
    Ok(normalized)
}
