// SPDX-License-Identifier: MIT

//! 10xer footer find: moves the cursor between name matches in the displayed
//! listing and keeps the matched substrings highlighted without hiding rows.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Range,
    path::PathBuf,
    rc::Weak,
};

use gtk::prelude::*;

use super::{BrowserView, ViewState};

/// The last committed find of a window. `n` repeats it after its highlights are
/// dismissed; leaving 10xer mode forgets it.
#[derive(Default)]
pub(super) struct FindState {
    query: String,
    backward: bool,
    highlighted: bool,
}

type Rgb = (u16, u16, u16);

thread_local! {
    static HIGHLIGHT_COLORS: Cell<Option<(Rgb, Rgb)>> = const { Cell::new(None) };
    static HIGHLIGHT_VIEWS: RefCell<Vec<Weak<ViewState>>> = const { RefCell::new(Vec::new()) };
}

fn rgb(color: &str) -> Option<Rgb> {
    let color = gtk::gdk::RGBA::parse(color).ok()?;
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
    Some((
        channel(color.red()),
        channel(color.green()),
        channel(color.blue()),
    ))
}

/// Pango needs concrete colors, so theme application passes its accent and
/// background here and live highlights are recolored.
pub(in crate::ui) fn apply_theme(accent: &str, background: &str) {
    HIGHLIGHT_COLORS.set(rgb(accent).zip(rgb(background)));
    let views = HIGHLIGHT_VIEWS.with_borrow_mut(|views| {
        views.retain(|view| view.strong_count() > 0);
        views.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
    });
    for view in views {
        view.refresh_name_highlights();
    }
}


pub(in crate::ui) fn match_ranges(name: &str, query: &str) -> Vec<Range<usize>> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    // A lowercase expansion keeps the byte span of the character it came from.
    let folded: Vec<(Range<usize>, char)> = name
        .char_indices()
        .flat_map(|(start, character)| {
            let span = start..start + character.len_utf8();
            character
                .to_lowercase()
                .map(move |lower| (span.clone(), lower))
        })
        .collect();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index + needle.len() <= folded.len() {
        let window = &folded[index..index + needle.len()];
        if window
            .iter()
            .zip(&needle)
            .all(|((_, character), wanted)| character == wanted)
        {
            ranges.push(window[0].0.start..window[needle.len() - 1].0.end);
            index += needle.len();
        } else {
            index += 1;
        }
    }
    ranges
}

fn highlight_attributes(text: &str, ranges: &[Range<usize>]) -> Option<gtk::pango::AttrList> {
    let mut ranges = ranges
        .iter()
        .filter(|range| {
            range.start < range.end
                && range.end <= text.len()
                && text.is_char_boundary(range.start)
                && text.is_char_boundary(range.end)
        })
        .peekable();
    ranges.peek()?;
    // A solid accent segment stays visible over the translucent accent of
    // selected rows. Before any theme is applied, bold still marks the match.
    let attributes = gtk::pango::AttrList::new();
    for range in ranges {
        let (start, end) = (range.start as u32, range.end as u32);
        let mut segment: Vec<gtk::pango::Attribute> = match HIGHLIGHT_COLORS.get() {
            Some(((ar, ag, ab), (br, bg, bb))) => vec![
                gtk::pango::AttrColor::new_background(ar, ag, ab).into(),
                gtk::pango::AttrColor::new_foreground(br, bg, bb).into(),
            ],
            None => vec![gtk::pango::AttrInt::new_weight(gtk::pango::Weight::Bold).into()],
        };
        for attribute in &mut segment {
            attribute.set_start_index(start);
            attribute.set_end_index(end);
        }
        for attribute in segment {
            attributes.insert(attribute);
        }
    }
    Some(attributes)
}

fn set_name_highlight(widget: &gtk::Widget, ranges: impl FnOnce(&str) -> Vec<Range<usize>>) {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        let text = label.text();
        let attributes = highlight_attributes(&text, &ranges(&text));
        if attributes.is_some() || label.attributes().is_some() {
            label.set_attributes(attributes.as_ref());
        }
    } else if let Some(label) = widget.downcast_ref::<gtk::Inscription>() {
        let text = label.text().unwrap_or_default();
        let attributes = highlight_attributes(&text, &ranges(&text));
        if attributes.is_some() || label.attributes().is_some() {
            label.set_attributes(attributes.as_ref());
        }
    }
}

pub(in crate::ui) fn highlight_name(widget: &gtk::Widget, query: Option<&str>) {
    set_name_highlight(widget, |text| {
        query.map_or_else(Vec::new, |query| match_ranges(text, query))
    });
}

/// Ranges index the displayed filename, not the full path.
pub(in crate::ui) type HitRanges = HashMap<PathBuf, Vec<Range<usize>>>;


pub(in crate::ui) fn highlight_listing_name(
    widget: &gtk::Widget,
    find: Option<&str>,
    filter: &str,
) {
    if find.is_some() {
        highlight_name(widget, find);
    } else {
        set_name_highlight(widget, |text| filter_term_ranges(text, filter));
    }
}

fn filter_term_ranges(name: &str, query: &str) -> Vec<Range<usize>> {
    if query.trim().is_empty() || !crate::ui::tenxer_mode::chrome_suppressed() {
        return Vec::new();
    }
    super::entry::with_filter_terms(query, |terms| terms.highlight(name, 0))
}

pub(in crate::ui) fn highlight_hit_name(
    widget: &gtk::Widget,
    find: Option<&str>,
    hit: Option<&[Range<usize>]>,
) {
    if find.is_some() {
        highlight_name(widget, find);
    } else {
        set_name_highlight(widget, |_| hit.map(<[_]>::to_vec).unwrap_or_default());
    }
}

impl ViewState {
    pub(in crate::ui) fn find_highlight(&self) -> Option<String> {
        let find = self.find.borrow();
        (find.highlighted && !find.query.is_empty()).then(|| find.query.clone())
    }

    pub(in crate::ui) fn refresh_name_highlights(&self) {
        let query = self.find_highlight();
        for column in self.columns.borrow().iter() {
            column.refresh_name_highlights(query.as_deref());
        }
        self.mode_views
            .borrow()
            .refresh_name_highlights(query.as_deref());
    }
}

impl BrowserView {


    #[cfg(test)]
    pub(in crate::ui) fn find_highlighted(&self) -> bool {
        self.state.find_highlight().is_some()
    }







}

#[cfg(test)]
mod tests;
