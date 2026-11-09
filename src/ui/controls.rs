// SPDX-License-Identifier: MIT

use std::{cell::Cell, ops::Range, rc::Rc, time::Duration};

use gtk::prelude::*;

pub(super) fn form_entry() -> gtk::Entry {
    let entry = gtk::Entry::new();
    entry.add_css_class("form-control");
    entry
}

#[cfg(test)]
mod tests;

pub(super) fn copyable_command(command: &str) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.add_css_class("preview-command");
    overlay.set_hexpand(true);

    let field = form_entry();
    field.add_css_class("preview-command-entry");
    field.set_text(command);
    field.set_editable(false);
    field.set_hexpand(true);
    overlay.set_child(Some(&field));

    let copy = gtk::Button::builder()
        .tooltip_text(crate::i18n::tr("Copy install command"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .build();
    copy.add_css_class("preview-command-copy");
    copy.set_has_frame(false);
    copy.set_cursor_from_name(Some("pointer"));
    let copy_icon = crate::assets::primary_icon(crate::assets::icons::COPY, 16);
    copy.set_child(Some(&copy_icon));
    let copied_command = command.to_owned();
    let feedback_generation = Rc::new(Cell::new(0_u64));
    copy.connect_clicked(move |button| {
        if let Some(display) = gtk::gdk::Display::default() {
            display.clipboard().set_text(&copied_command);
        }
        let generation = feedback_generation.get().saturating_add(1);
        feedback_generation.set(generation);
        crate::assets::set_primary_icon(&copy_icon, crate::assets::icons::CHECK);
        button.set_tooltip_text(Some(&crate::i18n::tr("Install command copied")));
        let button = button.clone();
        let copy_icon = copy_icon.clone();
        let feedback_generation = feedback_generation.clone();
        glib::timeout_add_local_once(Duration::from_secs(2), move || {
            if feedback_generation.get() == generation {
                crate::assets::set_primary_icon(&copy_icon, crate::assets::icons::COPY);
                button.set_tooltip_text(Some(&crate::i18n::tr("Copy install command")));
            }
        });
    });
    overlay.add_overlay(&copy);
    overlay
}

pub(super) fn form_password_entry() -> gtk::PasswordEntry {
    let entry = gtk::PasswordEntry::new();
    entry.add_css_class("form-control");
    entry
}

pub(super) fn form_error_label() -> gtk::Label {
    let label = gtk::Label::new(None);
    label.add_css_class("form-field-error");
    label.set_xalign(0.0);
    label.set_visible(false);
    label
}

pub(super) fn set_form_field_error(
    field: &impl IsA<gtk::Widget>,
    helper: &gtk::Label,
    message: Option<&str>,
) {
    if let Some(message) = message {
        field.add_css_class("error");
        helper.set_text(&crate::i18n::tr(message));
        helper.set_visible(true);
    } else {
        field.remove_css_class("error");
        helper.set_visible(false);
    }
}

pub(super) fn form_check_button(label: &str) -> gtk::CheckButton {
    let button = gtk::CheckButton::with_label(label);
    button.add_css_class("form-check");
    button
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ModalTone {
    #[default]
    Accent,
    Danger,
}

pub(super) const MESSAGE_DIALOG_WIDTH_CHARS: usize = 64;

/// Normalizes spacing within each line but keeps explicit line and paragraph breaks;
/// line wrapping is left to Pango so scripts without spaces break at valid positions.
pub(super) fn dialog_text(text: &str) -> String {
    let mut normalized = String::new();
    let mut pending_blank = false;
    for line in text.lines() {
        // No-break spaces (U+00A0, U+202F, U+2007) are typography, not spacing to normalize.
        let words = line
            .split([' ', '\t'])
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>();
        if words.is_empty() {
            pending_blank = !normalized.is_empty();
            continue;
        }
        if !normalized.is_empty() {
            normalized.push_str(if pending_blank { "\n\n" } else { "\n" });
        }
        normalized.push_str(&words.join(" "));
        pending_blank = false;
    }
    normalized
}

pub(super) fn message_dialog_description(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&dialog_text(text)));
    label.add_css_class("action-dialog-description");
    label.set_max_width_chars(MESSAGE_DIALOG_WIDTH_CHARS as i32);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    keep_words_whole(&label);
    label.set_xalign(0.0);
    label
}

// Longer runs of Hangul, such as file names, stay breakable so they cannot widen a dialog.
const MAX_KEPT_HANGUL_WORD_CHARS: usize = 12;

fn is_hangul_syllable(character: char) -> bool {
    ('\u{ac00}'..='\u{d7a3}').contains(&character)
}

/// Byte ranges of the short space-separated words that contain Hangul, and of the particle
/// ending a longer word with the character before it. Pango allows a line
/// break between any two Hangul syllables, but Korean wraps only between words.
pub(super) fn korean_word_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices().chain([(text.len(), ' ')]) {
        if !character.is_whitespace() {
            start.get_or_insert(index);
            continue;
        }
        let Some(word_start) = start.take() else {
            continue;
        };
        let word = &text[word_start..index];
        let length = word.chars().count();
        if length > 1
            && length <= MAX_KEPT_HANGUL_WORD_CHARS
            && word.chars().any(is_hangul_syllable)
        {
            ranges.push(word_start..index);
        } else if let Some(attached) = attached_particle_start(word) {
            ranges.push(word_start + attached..index);
        }
    }
    ranges
}

const MAX_PARTICLE_CHARS: usize = 6;

/// For a long word ending in a particle such as "을(를)", "(으)로" or "에서", the byte offset
/// of the character the particle attaches to, so the two stay on one line.
fn attached_particle_start(word: &str) -> Option<usize> {
    let is_particle_char = |character: char| {
        is_hangul_syllable(character) || matches!(character, '(' | ')' | ':' | ',' | '.')
    };
    let mut suffix_start = word.len();
    let mut suffix_chars = 0;
    for (index, character) in word.char_indices().rev() {
        if !is_particle_char(character) {
            break;
        }
        suffix_start = index;
        suffix_chars += 1;
    }
    let suffix = &word[suffix_start..];
    if suffix_chars > MAX_PARTICLE_CHARS {
        return parenthesised_particle_start(word);
    }
    if !suffix.chars().any(is_hangul_syllable) {
        return None;
    }
    previous_char_start(word, suffix_start)
}

fn previous_char_start(word: &str, index: usize) -> Option<usize> {
    word[..index]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

/// In a word that is all Hangul, a trailing "X(Y)" or "(X)Y" particle such as "을(를)" or
/// "(으)로" is the only recognizable boundary, so keep it with the syllable before it.
fn parenthesised_particle_start(word: &str) -> Option<usize> {
    let is_short_hangul = |text: &str, min: usize| {
        (min..=2).contains(&text.chars().count()) && text.chars().all(is_hangul_syllable)
    };
    let trimmed = word.trim_end_matches([':', ',', '.']);
    let close = trimmed.rfind(')')?;
    let open = trimmed[..close].rfind('(')?;
    let after = &trimmed[close + 1..];
    if !is_short_hangul(&trimmed[open + 1..close], 1) || !is_short_hangul(after, 0) {
        return None;
    }
    let particle = if after.is_empty() {
        previous_char_start(word, open)
            .filter(|&index| word[index..open].chars().all(is_hangul_syllable))?
    } else {
        open
    };
    previous_char_start(word, particle)
}

pub(super) fn no_break_attribute(range: Range<usize>) -> gtk::pango::Attribute {
    let mut attribute = gtk::pango::AttrInt::new_allow_breaks(false);
    attribute.set_start_index(range.start as u32);
    attribute.set_end_index(range.end as u32);
    attribute.into()
}

fn apply_word_break_attributes(label: &gtk::Label) {
    let attributes = label
        .attributes()
        .and_then(|attributes| attributes.copy())
        .unwrap_or_default();
    let _removed = attributes.filter(|attribute| {
        matches!(
            attribute.type_(),
            gtk::pango::AttrType::AllowBreaks | gtk::pango::AttrType::InsertHyphens
        )
    });
    attributes.insert(gtk::pango::AttrInt::new_insert_hyphens(false));
    for range in korean_word_ranges(&label.text()) {
        attributes.insert(no_break_attribute(range));
    }
    label.set_attributes(Some(&attributes));
}

/// Keeps a wrapping label from splitting Korean words or adding hyphens where Pango
/// has to break inside a word (as in a path before a Korean particle). The text itself,
/// and therefore the accessible name, stays unchanged.
pub(super) fn keep_words_whole(label: &gtk::Label) {
    apply_word_break_attributes(label);
    label.connect_label_notify(apply_word_break_attributes);
}

pub(super) struct ModalLayout {
    pub content: gtk::Box,
    pub body: gtk::Box,
    pub actions: gtk::Box,
    pub title: gtk::Label,
    pub subtitle: gtk::Label,
    pub loading: gtk::Spinner,
    pub close: gtk::Button,
    pub cancel: gtk::Button,
    pub confirm: gtk::Button,
    pub icon: gtk::Image,
}

impl ModalLayout {
    pub fn set_loading(&self, loading: bool, description: Option<&str>) {
        set_modal_loading(&self.loading, loading, description);
    }
}

fn modal_loading_spinner() -> gtk::Spinner {
    let loading = gtk::Spinner::new();
    loading.add_css_class("action-dialog-loading");
    loading.set_visible(false);
    loading
}

/// [`ModalLayout::set_loading`] for a dialog that keeps only its spinner.
pub(super) fn set_modal_loading(spinner: &gtk::Spinner, loading: bool, description: Option<&str>) {
    if loading {
        let description = description.map_or_else(|| crate::i18n::tr("Working…"), str::to_owned);
        crate::ui::accessibility::set_description(spinner, Some(&description));
        spinner.set_visible(true);
        spinner.start();
    } else {
        spinner.stop();
        spinner.set_visible(false);
        crate::ui::accessibility::set_description(spinner, None);
    }
}

/// Builds the shared structure and styling for an action modal from already translated text.
pub(super) fn modal_layout(
    icon: &str,
    title: &str,
    subtitle: &str,
    confirm_label: &str,
) -> ModalLayout {
    modal_layout_with_tone(icon, title, subtitle, confirm_label, ModalTone::Accent)
}

pub(super) fn modal_layout_with_tone(
    icon: &str,
    title: &str,
    subtitle: &str,
    confirm_label: &str,
    tone: ModalTone,
) -> ModalLayout {
    let content = super::accessibility::dialog_box(title);
    content.add_css_class("action-dialog");
    content.set_halign(gtk::Align::Center);
    content.set_valign(gtk::Align::Center);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("action-dialog-header");
    header.set_valign(gtk::Align::Center);

    // 2.0 dialogs lead with their title; the icon stays for callers that swap it.
    let icon = match tone {
        ModalTone::Accent => crate::assets::primary_icon(icon, 18),
        ModalTone::Danger => crate::assets::danger_icon(icon, 18),
    };
    icon.set_visible(false);

    let heading = gtk::Box::new(gtk::Orientation::Vertical, 1);
    heading.add_css_class("action-dialog-heading");
    heading.set_hexpand(true);
    heading.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(title));
    title.add_css_class("action-dialog-title");
    title.set_wrap(true);
    title.set_xalign(0.0);
    let subtitle = gtk::Label::new(Some(subtitle));
    subtitle.add_css_class("action-dialog-subtitle");
    subtitle.set_xalign(0.0);
    heading.append(&title);
    heading.append(&subtitle);

    let loading = modal_loading_spinner();

    let close = nyx_ui::widgets::bar_button(
        crate::assets::icons::X,
        &crate::i18n::tr("Close dialog"),
    );
    close.add_css_class("action-dialog-close");
    super::accessibility::set_own_label(&close, &crate::i18n::tr("Close dialog"));

    header.append(&icon);
    header.append(&heading);
    header.append(&loading);
    header.append(&close);
    content.append(&header);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.add_css_class("action-dialog-body");
    content.append(&body);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.add_css_class("action-dialog-actions");
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let cancel = gtk::Button::with_label(&crate::i18n::tr("Cancel"));
    cancel.add_css_class("action-dialog-cancel");
    let confirm = gtk::Button::with_label(confirm_label);
    confirm.add_css_class("action-dialog-confirm");
    confirm.add_css_class(match tone {
        ModalTone::Accent => "suggested-action",
        ModalTone::Danger => "destructive-action",
    });
    actions.append(&spacer);
    actions.append(&cancel);
    actions.append(&confirm);
    content.append(&actions);

    ModalLayout {
        content,
        body,
        actions,
        title,
        subtitle,
        loading,
        close,
        cancel,
        confirm,
        icon,
    }
}

/// Builds a single-selection group with the same compact treatment as a segmented control.
pub(super) fn segmented_control(
    labels: &[&str],
    selected: usize,
) -> (gtk::Box, Vec<gtk::ToggleButton>) {
    let control = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    control.set_homogeneous(true);
    control.add_css_class("segmented-control");
    control.add_css_class("segmented");

    let mut buttons = Vec::with_capacity(labels.len());
    for (index, label) in labels.iter().enumerate() {
        let button = gtk::ToggleButton::with_label(&crate::i18n::tr(label));
        if let Some(label) = button.child().and_downcast::<gtk::Label>() {
            // Wrapping containers may wrap choices, but never inside a word.
            label.add_css_class("segmented-control-label");
            label.set_wrap_mode(gtk::pango::WrapMode::Word);
        }
        button.add_css_class("segmented-control-option");
        button.add_css_class("segment");
        button.set_hexpand(true);
        if let Some(first) = buttons.first() {
            button.set_group(Some(first));
        }
        button.set_active(index == selected);
        control.append(&button);
        buttons.push(button);
    }

    (control, buttons)
}
