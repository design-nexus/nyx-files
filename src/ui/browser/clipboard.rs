// SPDX-License-Identifier: MIT

use crate::adapters::{
    DropVolumeQuery, DropVolumes, gio_file_for_location, location_for_file, lookup_drop_volumes,
};
use crate::model::{FileEntry, Location};
use crate::services::{
    CrossVolumeDropStrategy, DropActionInput, DropCommit, DropOverride, TransferKind,
    VolumeRelation, drop_commit, transferable_drop_sources,
};
use crate::ui::browser::ViewState;
use crate::ui::browser::paths::{is_trash_location};
use gtk::prelude::*;
use gtk::{glib, graphene};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::Duration;

const DRAG_PROXY_PADDING: f64 = 3.0;
const DRAG_PROXY_STACK_OFFSET: f64 = 5.0;

const DRAG_PREVIEW_ICON_PX: f64 = 32.0;

struct DragPreviewLayout {
    canvas_w: f64,
    canvas_h: f64,
    front_x: f64,
    front_y: f64,
    badge: Option<(f64, f64, f64, f64)>,
    hotspot: (i32, i32),
}

fn drag_preview_layout(icon_px: f64, badge: Option<(f64, f64)>) -> DragPreviewLayout {
    let front_x = DRAG_PROXY_PADDING;
    let front_y = DRAG_PROXY_PADDING;
    let (badge_x, badge_y, badge_w, badge_h) = badge.map_or((0.0, 0.0, 0.0, 0.0), |(w, h)| {
        (
            front_x + icon_px - w * 0.4,
            front_y + icon_px - h * 0.4,
            w,
            h,
        )
    });
    let rear_extent = DRAG_PROXY_STACK_OFFSET + DRAG_PROXY_PADDING;
    let canvas_w = (badge_x + badge_w)
        .max(front_x + icon_px)
        .max(icon_px + rear_extent + DRAG_PROXY_PADDING);
    let canvas_h = (badge_y + badge_h)
        .max(front_y + icon_px)
        .max(icon_px + rear_extent + DRAG_PROXY_PADDING);
    DragPreviewLayout {
        canvas_w: canvas_w + DRAG_PROXY_PADDING,
        canvas_h: canvas_h + DRAG_PROXY_PADDING,
        front_x,
        front_y,
        badge: badge.map(|_| (badge_x, badge_y, badge_w, badge_h)),
        hotspot: (
            (front_x + icon_px / 2.0).round() as i32,
            (front_y + icon_px / 2.0).round() as i32,
        ),
    }
}

#[expect(
    deprecated,
    reason = "lookup_color is the only way to read custom named CSS colors"
)]
pub(crate) fn drag_preview_icon(
    base: &impl IsA<gtk::Widget>,
    entries: &[FileEntry],
) -> Option<(gtk::gdk::Texture, i32, i32)> {
    let icon_name = entries
        .first()
        .map(super::entry::entry_icon)
        .unwrap_or(crate::assets::icons::DOCUMENTS);
    let icon = crate::assets::drag_icon_texture(
        icon_name,
        &crate::assets::primary_icon_color(),
        DRAG_PREVIEW_ICON_PX as i32,
    )?;
    let icon_w = f64::from(icon.width()).max(1.0);
    let icon_h = f64::from(icon.height()).max(1.0);

    let style = base.style_context();
    let accent = style.lookup_color("strata_accent")?;
    let surface = style
        .lookup_color("strata_surface")
        .or_else(|| style.lookup_color("strata_bg"))?;
    let text = style.lookup_color("strata_text")?;
    let badge_text = contrasting_badge_text(&accent, &text, &surface);

    let multiple = entries.len() > 1;
    let badge = multiple.then(|| {
        let layout = base.create_pango_layout(Some(&entries.len().to_string()));
        if let Some(mut font) = layout.font_description() {
            font.set_weight(gtk::pango::Weight::Semibold);
            layout.set_font_description(Some(&font));
        }
        let (ink, _) = layout.pixel_extents();
        let (w, h) = badge_dimensions(f64::from(ink.width()), f64::from(ink.height()));
        (layout, badge_text, ink, w, h)
    });
    let layout = drag_preview_layout(
        icon_w.max(icon_h),
        badge.as_ref().map(|(_, _, _, w, h)| (*w, *h)),
    );
    let icon_rect =
        |x: f64, y: f64| graphene::Rect::new(x as f32, y as f32, icon_w as f32, icon_h as f32);

    let snapshot = gtk::Snapshot::new();
    let transparent = gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
    snapshot.append_color(
        &transparent,
        &graphene::Rect::new(0.0, 0.0, layout.canvas_w as f32, layout.canvas_h as f32),
    );

    if multiple {
        for (offset, opacity) in [(DRAG_PROXY_STACK_OFFSET, 0.32), (2.5, 0.6)] {
            snapshot.push_opacity(opacity);
            snapshot.append_texture(
                &icon,
                &icon_rect(layout.front_x + offset, layout.front_y + offset),
            );
            snapshot.pop();
        }
    }

    let mut shadow_color = text;
    shadow_color.set_alpha(0.34);
    snapshot.push_shadow(&[gtk::gsk::Shadow::new(shadow_color, 0.0, 1.0, 3.0)]);
    snapshot.append_texture(&icon, &icon_rect(layout.front_x, layout.front_y));
    snapshot.pop();

    if let (Some((badge_x, badge_y, badge_w, badge_h)), Some((pango, color, ink, _, _))) =
        (layout.badge, badge.as_ref())
    {
        let badge_rect = gtk::gsk::RoundedRect::from_rect(
            graphene::Rect::new(
                badge_x as f32,
                badge_y as f32,
                badge_w as f32,
                badge_h as f32,
            ),
            (badge_h / 2.0) as f32,
        );
        snapshot.push_rounded_clip(&badge_rect);
        snapshot.append_color(&accent, badge_rect.bounds());
        snapshot.pop();
        snapshot.append_border(
            &badge_rect,
            &[1.0; 4],
            &[surface, surface, surface, surface],
        );
        let tx = badge_x + (badge_w - f64::from(ink.width())) / 2.0 - f64::from(ink.x());
        let ty = badge_y + (badge_h - f64::from(ink.height())) / 2.0 - f64::from(ink.y());
        snapshot.save();
        snapshot.translate(&graphene::Point::new(tx as f32, ty as f32));
        snapshot.append_layout(pango, color);
        snapshot.restore();
    }

    let renderer = base.native().and_then(|native| native.renderer())?;
    let node = snapshot.to_node()?;
    let texture = renderer.render_texture(&node, None);
    Some((texture, layout.hotspot.0, layout.hotspot.1))
}

fn badge_dimensions(text_width: f64, text_height: f64) -> (f64, f64) {
    let height = (text_height + 6.0).max(20.0);
    ((text_width + 10.0).max(height), height)
}

fn contrasting_badge_text(
    fill: &gtk::gdk::RGBA,
    text: &gtk::gdk::RGBA,
    surface: &gtk::gdk::RGBA,
) -> gtk::gdk::RGBA {
    if contrast_ratio(fill, text) >= contrast_ratio(fill, surface) {
        *text
    } else {
        *surface
    }
}

fn contrast_ratio(first: &gtk::gdk::RGBA, second: &gtk::gdk::RGBA) -> f64 {
    let first = relative_luminance(first);
    let second = relative_luminance(second);
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}

fn relative_luminance(color: &gtk::gdk::RGBA) -> f64 {
    let linear = |channel: f32| {
        let channel = f64::from(channel);
        if channel <= 0.03928 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.red()) + 0.7152 * linear(color.green()) + 0.0722 * linear(color.blue())
}


#[derive(Clone)]
pub(crate) struct DragSnapshot {
    pub locations: Rc<[Location]>,
    /// Locations with display names, present only when every entry is a directory.
    pub folders: Option<Rc<[(Location, String)]>>,
}

struct ActiveDrag {
    content: glib::WeakRef<gtk::gdk::ContentProvider>,
    snapshot: DragSnapshot,
}

thread_local! {
    static ACTIVE_DRAG: RefCell<Option<ActiveDrag>> = const { RefCell::new(None) };
}




pub(crate) fn clear_active_drag() {
    ACTIVE_DRAG.with(|cell| {
        cell.borrow_mut().take();
    });
}





























pub(in crate::ui) fn file_drag_content(entries: &[FileEntry]) -> Option<gtk::gdk::ContentProvider> {
    if entries.is_empty() {
        return None;
    }
    let files = entries
        .iter()
        .map(|entry| gio_file_for_location(&entry.location))
        .collect::<Vec<_>>();
    let file_list =
        gtk::gdk::ContentProvider::for_value(&gtk::gdk::FileList::from_array(&files).to_value());
    let mut uri_list = String::with_capacity(files.len() * 48);
    for file in &files {
        uri_list.push_str(&file.uri());
        uri_list.push_str("\r\n");
    }
    let uri_list = gtk::gdk::ContentProvider::for_bytes(
        "text/uri-list",
        &glib::Bytes::from_owned(uri_list.into_bytes()),
    );
    let content = gtk::gdk::ContentProvider::new_union(&[file_list, uri_list]);
    let locations = entries.iter().map(|entry| entry.location.clone()).collect();
    ACTIVE_DRAG.with(|cell| {
        *cell.borrow_mut() = Some(ActiveDrag {
            content: content.downgrade(),
            snapshot: DragSnapshot {
                locations,
                folders: entries.iter().all(FileEntry::is_directory).then(|| {
                    entries
                        .iter()
                        .map(|entry| (entry.location.clone(), entry.display_name.clone()))
                        .collect()
                }),
            },
        });
    });
    Some(content)
}









#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ClipboardMark {
    #[default]
    None,
    Copy,
    Cut,
}

pub(crate) type ClipboardMarks = HashMap<Location, ClipboardMark>;


// Process-wide copy/cut intent shared by every window. The GDK clipboard only
// carries a `FileList` with no cut marker, so this thread-local (GTK stays on
// the main thread) is the source of truth for both paste behavior and styling.
thread_local! {
    static SHARED_CUT_LOCATIONS: RefCell<Vec<Location>> = const { RefCell::new(Vec::new()) };
    static SHARED_COPY_LOCATIONS: RefCell<Vec<Location>> = const { RefCell::new(Vec::new()) };
    static SHARED_MARK_INDEX: RefCell<ClipboardMarks> = RefCell::new(ClipboardMarks::new());
    static OWNED_FILE_PROVIDER: RefCell<Option<gtk::gdk::ContentProvider>> = const { RefCell::new(None) };
    static CUT_VIEWS: RefCell<Vec<Weak<ViewState>>> = const { RefCell::new(Vec::new()) };
    static WATCHING_OWNERSHIP: Cell<bool> = const { Cell::new(false) };
}













#[cfg(test)]
fn set_shared_cut(locations: &[Location]) {
    set_shared_marks(&[], locations);
}











impl ViewState {







}



