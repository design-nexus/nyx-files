// SPDX-License-Identifier: MIT

//! Storage properties, Files-only display labels, and removable-only formatting.


use gtk::{gio, prelude::*};



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

#[cfg(test)]
mod tests;
