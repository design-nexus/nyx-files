// SPDX-License-Identifier: MIT

//! Storage properties, Files-only display labels, and removable-only formatting.

use std::{cell::Cell, future::Future, path::PathBuf, rc::Rc};

use gtk::{gio, glib, prelude::*};

use crate::{
    assets,
    services::package_manager::PackageManager,
    ui::{
        controls::{
            ModalLayout, ModalTone, form_check_button, keep_words_whole, modal_layout, },
        
        modal::{
            ModalHost, close_on_escape, dismiss_modal_layer, modal_layer, remember_modal_focus,
        },
    },
};

use super::drive_ops::{self, FilesystemType};

mod format_progress;


#[derive(Clone)]
pub(crate) enum PropertiesTarget {
    Volume(gio::Volume),
    Mount(gio::Mount),
}

impl PropertiesTarget {
    pub(crate) fn volume(&self) -> Option<gio::Volume> {
        match self {
            Self::Volume(volume) => Some(volume.clone()),
            Self::Mount(mount) => mount.volume(),
        }
    }

    pub(crate) fn label_id(&self) -> Option<String> {
        let uuid = self
            .volume()
            .and_then(|volume| volume.uuid())
            .or_else(|| self.mount().and_then(|mount| mount.uuid()));
        let uri = self.mount().map(|mount| mount.root().uri());
        super::devices::label_identity(uuid.as_deref(), uri.as_deref())
    }

    fn mount(&self) -> Option<gio::Mount> {
        match self {
            Self::Volume(volume) => volume.get_mount(),
            Self::Mount(mount) => Some(mount.clone()),
        }
    }

}





fn field_block(label_text: &str, field: &impl IsA<gtk::Widget>) -> gtk::Box {
    let block = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let label = gtk::Label::new(Some(&crate::i18n::tr(label_text)));
    label.set_xalign(0.0);
    block.append(&label);
    field.set_hexpand(true);
    block.append(field);
    block
}
































#[cfg(test)]
mod tests;
