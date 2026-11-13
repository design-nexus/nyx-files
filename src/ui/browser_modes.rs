// SPDX-License-Identifier: MIT

//! The listing's views (Columns, Icons, List), how clicks open items, its
//! densities, and the Icons thumbnail sizes.

use std::path::Path;

use crate::model::{FileEntry, MetadataValue};

pub(crate) mod thumbnail_size;
pub(crate) use thumbnail_size::ThumbnailResize;

pub(crate) const DEFAULT_ICONS_THUMBNAIL_SIZE: i32 = 112;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrowserMode {
    #[default]
    Columns,
    Icons,
    List,
}

/// One position on a density slider: a px value, where `COMPACT_PX` and `AIRY_PX`
/// are the snap points and every other value in `0..=MAX_PX` is Custom.
pub(crate) trait DensityScale: Copy + PartialEq + 'static {
    const COMPACT_PX: u8;
    const AIRY_PX: u8;
    const MAX_PX: u8;
    fn from_px(px: u8) -> Self;
    fn px(self) -> u8;
}

macro_rules! density_scale {
    ($(#[$meta:meta])* $name:ident { compact: $compact:literal, airy: $airy:literal, max: $max:literal }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub enum $name {
            #[default]
            Compact,
            Airy,
            /// `from_px` maps snap-point values to their preset rather than to Custom.
            Custom(u8),
        }

        impl DensityScale for $name {
            const COMPACT_PX: u8 = $compact;
            const AIRY_PX: u8 = $airy;
            const MAX_PX: u8 = $max;

            fn from_px(px: u8) -> Self {
                match px.min(Self::MAX_PX) {
                    $compact => Self::Compact,
                    $airy => Self::Airy,
                    px => Self::Custom(px),
                }
            }

            fn px(self) -> u8 {
                match self {
                    Self::Compact => Self::COMPACT_PX,
                    Self::Airy => Self::AIRY_PX,
                    Self::Custom(px) => px.min(Self::MAX_PX),
                }
            }
        }
    };
}

density_scale!(
    /// File-row and icon-card vertical spacing in logical px.
    BrowserDensity { compact: 4, airy: 8, max: 16 }
);

density_scale!(
    /// Extra vertical inset shared by the header, tab strip, sidebar rows, and
    /// Miller/preview headers, so their heights stay in lock step.
    FrameDensity { compact: 0, airy: 4, max: 12 }
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClickCount {
    One,
    Two,
}

impl ClickCount {
    pub fn from_stored(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            _ => None,
        }
    }

    pub fn stored(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClickActivation {
    pub files: ClickCount,
    pub folders: ClickCount,
}

impl ClickActivation {
    pub fn default_for(mode: BrowserMode) -> Self {
        Self {
            files: ClickCount::Two,
            folders: match mode {
                BrowserMode::Columns => ClickCount::One,
                BrowserMode::Icons | BrowserMode::List => ClickCount::Two,
            },
        }
    }
}

impl Default for ClickActivation {
    fn default() -> Self {
        Self::default_for(BrowserMode::Columns)
    }
}

pub(crate) fn icon_details_need_fill(entry: &FileEntry) -> bool {
    let path = Path::new(&entry.native_name);
    (entry.is_directory() && entry.child_count == MetadataValue::Unknown)
        || (!entry.is_directory()
            && ((entry.image_dimensions == MetadataValue::Unknown
                && crate::services::is_image_path(path))
                || (entry.duration_seconds == MetadataValue::Unknown
                    && crate::services::is_media_path(path))))
}
