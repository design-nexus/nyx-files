// SPDX-License-Identifier: MIT

//! Customize: a folder's or file's colour, an icon or an emoji on it.

use std::path::PathBuf;

use gtk::{gdk, prelude::*};
use nyx_ui::widgets;

use crate::{
    assets::icons,
    model::{FolderColor, FolderColorValue},
    ui::preferences::PreferenceManager,
};

fn hex(rgba: &gdk::RGBA) -> String {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        c(rgba.red()),
        c(rgba.green()),
        c(rgba.blue())
    )
}

pub(super) fn show(path: PathBuf, directory: bool, name: &str) {
    let fallback = if directory {
        icons::FOLDER
    } else {
        icons::DOCUMENTS
    };
    let title = crate::i18n::tr(if directory {
        "Customize Folder"
    } else {
        "Customize File"
    });
    let (dialog, card) = widgets::dialog(&title, 440);
    card.add_css_class("customize-card");
    let head = widgets::hbox(12);
    let preview = gtk::Image::new();
    preview.set_pixel_size(56);
    crate::ui::thumbnail::show_customized_icon_image(&preview, &path, fallback, 56);
    head.append(&preview);
    head.append(&widgets::label(name, "properties-name"));
    card.append(&head);
    let refresh = {
        let (preview, path) = (preview.clone(), path.clone());
        move || crate::ui::thumbnail::show_customized_icon_image(&preview, &path, fallback, 56)
    };

    card.append(&widgets::label(
        &crate::i18n::tr("Color").to_uppercase(),
        "menu-heading",
    ));
    let colors = widgets::hbox(6);
    colors.add_css_class("customize-colors");
    for color in FolderColor::ALL {
        let swatch = gtk::Button::new();
        swatch.add_css_class("customize-swatch");
        swatch.set_size_request(26, 26);
        swatch.set_valign(gtk::Align::Center);
        swatch.set_tooltip_text(Some(&crate::i18n::tr(color.name())));
        widgets::paint(&swatch, color.hex());
        let (p, r) = (path.clone(), refresh.clone());
        swatch.connect_clicked(move |_| {
            PreferenceManager::shared().set_folder_color(&p, Some(FolderColorValue::Preset(color)));
            r();
        });
        colors.append(&swatch);
    }
    let custom = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    custom.set_tooltip_text(Some(&crate::i18n::tr("Custom color")));
    if let Some(FolderColorValue::Custom(value)) = PreferenceManager::shared().folder_color(&path)
        && let Ok(rgba) = gdk::RGBA::parse(value.as_str())
    {
        custom.set_rgba(&rgba);
    }
    let (p, r) = (path.clone(), refresh.clone());
    custom.connect_rgba_notify(move |b| {
        PreferenceManager::shared()
            .set_folder_color(&p, Some(FolderColorValue::Custom(hex(&b.rgba()))));
        r();
    });
    colors.append(&custom);
    card.append(&colors);

    card.append(&widgets::label(
        &crate::i18n::tr("Icon").to_uppercase(),
        "menu-heading",
    ));
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_min_children_per_line(6);
    grid.set_max_children_per_line(8);
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    for (icon, label) in icons::CUSTOMIZATION_CHOICES {
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.add_css_class("customize-icon");
        button.set_tooltip_text(Some(&crate::i18n::tr(label)));
        button.set_child(Some(&crate::assets::primary_icon(icon, 20)));
        let (p, r) = (path.clone(), refresh.clone());
        button.connect_clicked(move |_| {
            PreferenceManager::shared().set_custom_icon(&p, Some(icon));
            r();
        });
        grid.append(&button);
    }
    card.append(&grid);
    let emoji = gtk::Button::with_label(&crate::i18n::tr("Choose Emoji…"));
    let chooser = gtk::EmojiChooser::new();
    chooser.set_parent(&emoji);
    let c = chooser.clone();
    emoji.connect_clicked(move |_| c.popup());
    let (p, r) = (path.clone(), refresh.clone());
    chooser.connect_emoji_picked(move |_, picked| {
        PreferenceManager::shared().set_custom_icon(&p, Some(&format!("emoji:{picked}")));
        r();
    });
    card.append(&emoji);

    let buttons = widgets::hbox(8);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(6);
    let clear = gtk::Button::with_label(&crate::i18n::tr("Reset"));
    let done = gtk::Button::with_label(&crate::i18n::tr("Done"));
    done.add_css_class("suggested-action");
    buttons.append(&clear);
    buttons.append(&done);
    card.append(&buttons);
    let (p, r) = (path, refresh);
    clear.connect_clicked(move |_| {
        PreferenceManager::shared().clear_item_customization(&p);
        r();
    });
    let d = dialog.clone();
    done.connect_clicked(move |_| d.close());
    dialog.connect_close_request(move |_| {
        chooser.unparent();
        gtk::glib::Propagation::Proceed
    });
    dialog.present();
    done.grab_focus();
}
