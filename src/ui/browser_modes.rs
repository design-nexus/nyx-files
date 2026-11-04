// SPDX-License-Identifier: MIT

//! Alternate browser presentations.
//!
//! This module is deliberately isolated from the Miller-column implementation. It consumes the
//! same application events and emits the same navigation/selection intents, so adding another
//! presentation does not require scattering mode checks throughout the main browser view.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::Path,
    rc::{Rc, Weak},
};

use gtk::{gio, glib, prelude::*};

use crate::{
    app::Browser,
    model::{FileEntry, Location, MetadataValue, SortDirection, SortKey},
    services::DropCommit,
    ui::browser::ClipboardMarks,
};


mod column_options;
mod events;
mod list_factory;
mod navigation;

pub(crate) mod thumbnail_size;
pub(crate) use thumbnail_size::ThumbnailResize;

pub(crate) const DEFAULT_ICONS_THUMBNAIL_SIZE: i32 = 64;

#[derive(Clone)]
struct ListColumnLayout {
    widths: Rc<Vec<Cell<i32>>>,
    cells: Rc<Vec<RefCell<Vec<glib::WeakRef<gtk::Widget>>>>>,
    name_manually_resized: Rc<Cell<bool>>,
    scale: Rc<Cell<f64>>,
    chooser: bool,
}

impl ListColumnLayout {

}

type TransferHandler = Rc<dyn Fn(Location, Vec<Location>, DropCommit)>;
type TransferHandlerSlot = Rc<RefCell<Option<TransferHandler>>>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BrowserMode {
    #[default]
    Columns,
    Icons,
    List,
}

impl BrowserMode {
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

// Capture must snapshot selection before GTK changes it for the bubble gesture.

/// A view's source positions by view position, built for the two maps' generations.
type ViewSources = ((u64, u64), Vec<u32>);

/// Filter, sort, and flatten models preserve item identity across position changes.
#[derive(Clone, Default)]
struct SourceIndexMap {
    by_item: Rc<RefCell<HashMap<glib::Object, usize>>>,
    generation: Rc<Cell<u64>>,
    // Reading a filter model's item walks its match set, so selections map through this.
    sources: Rc<RefCell<Option<ViewSources>>>,
}

impl SourceIndexMap {



    fn of_item(&self, item: &glib::Object) -> Option<usize> {
        self.by_item.borrow().get(item).copied()
    }

    fn of_view_position(&self, view: &impl IsA<gio::ListModel>, position: u32) -> Option<usize> {
        view.item(position).and_then(|item| self.of_item(&item))
    }

    /// Positions in `source` of this map's `view` items at `positions`.
    fn sources_of(
        &self,
        source: &SourceIndexMap,
        view: &gio::ListModel,
        positions: impl Iterator<Item = u32>,
    ) -> Vec<Option<usize>> {
        const UNMAPPED: u32 = u32::MAX;
        let key = (self.generation.get(), source.generation.get());
        let mut cache = self.sources.borrow_mut();
        let forward = match &mut *cache {
            Some((built, forward)) if *built == key => forward,
            slot => {
                let mut forward = vec![UNMAPPED; view.n_items() as usize];
                let by_source_item = source.by_item.borrow();
                for (item, position) in self.by_item.borrow().iter() {
                    if let Some(slot) = forward.get_mut(*position)
                        && let Some(source) = by_source_item
                            .get(item)
                            .and_then(|source| u32::try_from(*source).ok())
                    {
                        *slot = source;
                    }
                }
                &mut slot.insert((key, forward)).1
            }
        };
        positions
            .map(|position| match forward.get(position as usize) {
                Some(&source) if source != UNMAPPED => Some(source as usize),
                _ => source.of_view_position(view, position),
            })
            .collect()
    }
}

struct BoundModeItem {
    item: glib::WeakRef<gtk::ListItem>,
    widget: glib::WeakRef<gtk::Widget>,
    rename_label: glib::WeakRef<gtk::Widget>,
    edit: super::collection_edit::EditWidgets,
}

/// One collection view inside a pane. Icons and List each keep a single section.
#[derive(Clone)]
struct PaneSection {
    view: gtk::Widget,
    view_model: gio::ListModel,
    view_index: SourceIndexMap,
    selection: gtk::MultiSelection,
    bound_items: Rc<RefCell<Vec<BoundModeItem>>>,
    syncing: Rc<Cell<bool>>,
    visit: super::marquee::ItemVisitor,
    item_context_trigger: Rc<dyn Fn(f64, f64)>,
}

impl PaneSection {
    fn source_to_view(&self, source: &gtk::StringList, position: usize) -> Option<u32> {
        self.view_index
            .of_item(&source.item(position as u32)?)
            .map(|position| position as u32)
    }
}

type ListSorting = Rc<HeadingSort>;

struct HeadingSort {
    current: Cell<(SortKey, SortDirection)>,
    arrows: RefCell<Vec<(SortKey, gtk::Image)>>,
}

impl HeadingSort {

}

#[derive(Clone)]
struct Pane {
    depth: usize,
    location: Option<Location>,
    group_by_type: bool,
    type_group_sorter: Option<TypeGroupSorter>,
    sorting: Option<ListSorting>,
    sort_direction_button: Option<gtk::Button>,
    shell: gtk::Box,
    header: gtk::Box,
    model: gtk::StringList,
    source_index: SourceIndexMap,
    filter_model: Option<gtk::FilterListModel>,
    /// The primary section that owns the pane's chrome.
    section: PaneSection,
    sections: Rc<RefCell<Vec<PaneSection>>>,
    icons: Option<Rc<IconsContext>>,
    thumbnail_scale: Option<gtk::Scale>,
    targets: super::marquee::MarqueeTargets,
    /// Set while a reload has detached the pane's models from their views.
    detached: Rc<Cell<bool>>,
    stack: gtk::Stack,
    loading: super::loading_skeleton::DelayedLoading,
    status: gtk::Label,
    spinner: gtk::Spinner,
    truncated_hint: gtk::Image,
    not_responding: gtk::Box,
    marquee: super::marquee::Marquee,
    search: super::inline_search::InlineSearch,
    filter_entry: Option<gtk::Entry>,
    filter_button: Option<gtk::ToggleButton>,
    empty_trash_button: Option<gtk::Button>,
    leftovers_button: gtk::Button,
    show_hidden: Rc<Cell<bool>>,
    filter: gtk::CustomFilter,
    filter_query: Rc<RefCell<String>>,
    folder_context_trigger: Rc<dyn Fn(f64, f64)>,
    list_headings: Option<gtk::Box>,
    list_columns: Option<ListColumnLayout>,
}

impl Pane {
    /// The sections that render entries, in visual order.
    fn item_sections(&self) -> Vec<PaneSection> {
        self.sections.borrow().clone()
    }


}

/// Where the List or Icons pane's row joins the window header: the row stays one
/// container (keyboard focus moves within it), and the path control takes the
/// place of its folder heading. Columns mode gives the path back, and so does a
/// pane row that 10xer mode hides.
pub struct HeaderHost {
    row: gtk::Box,
    after: gtk::Widget,
    location: gtk::Widget,
    adopted: RefCell<Option<glib::WeakRef<gtk::Box>>>,
    visibility: RefCell<Option<glib::SignalHandlerId>>,
}

impl HeaderHost {




}


pub struct ModeViews {
    header_host: RefCell<Option<Rc<HeaderHost>>>,
    stack: gtk::Stack,
    icons_root: gtk::Box,
    list_root: gtk::Box,
    icons_panes: Vec<Pane>,
    list_pane: Option<Pane>,
    pane_navigation: RefCell<navigation::PaneNavigation>,
    browser: Rc<Browser>,
    single_click_previews: Rc<Cell<bool>>,
    multiple_selection: Rc<Cell<bool>>,
    icons_click_activation: Rc<Cell<ClickActivation>>,
    list_click_activation: Rc<Cell<ClickActivation>>,
    transfer_handler: TransferHandlerSlot,
    clipboard_marks: Rc<RefCell<ClipboardMarks>>,
    context_state: Rc<RefCell<Option<Weak<super::browser::ViewState>>>>,
    new_folder_state: RefCell<Option<Weak<super::browser::ViewState>>>,
    active_rename: Rc<RefCell<Option<super::collection_edit::ActiveEdit>>>,
    search_selection_handlers: super::inline_search::SearchSelectionHandlers,
    mode: BrowserMode,
    density: BrowserDensity,
    group_by_type: bool,
    icons_thumbnail_size: Rc<Cell<i32>>,
    /// The size preferences gave the shown folder; a size changed on a folder
    /// that is not remembered lasts until this changes.
    icons_resolved_size: Cell<i32>,
    focus_before_header: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    /// Page Up/Down scrolls the viewport itself; skip the follow-up `scroll_to`
    /// that `FocusChanged` would otherwise schedule from stale GridView estimates.
    suppress_focus_scroll: Cell<bool>,
    /// Set while a footer prompt moves the cursor; the prompt keeps the keys.
    cursor_keeps_focus: Cell<bool>,
    /// A pane whose results, rather than its hidden listing, take the focus a
    /// `FocusChanged` asked for. The view state focuses them on their cursor.
    results_take_focus: Cell<Option<usize>>,
}

impl ModeViews {



    pub fn mode(&self) -> BrowserMode {
        self.mode
    }




    fn single_pane(&self) -> Option<&Pane> {
        match self.mode {
            BrowserMode::Columns => None,
            BrowserMode::Icons => self.icons_panes.first(),
            BrowserMode::List => self.list_pane.as_ref(),
        }
    }









    pub fn selected_positions(&self) -> Option<(usize, Vec<usize>)> {
        let pane = match self.mode {
            BrowserMode::Columns => return None,
            BrowserMode::Icons => self.icons_panes.first(),
            BrowserMode::List => self.list_pane.as_ref(),
        }?;
        let mut positions: Vec<usize> = pane
            .item_sections()
            .iter()
            .flat_map(|section| selected_source_positions(&pane.source_index, section))
            .collect();
        positions.sort_unstable();
        positions.dedup();
        Some((pane.depth, positions))
    }



















    pub fn selected_search_result(&self) -> Option<FileEntry> {
        self.single_pane()?.search.selected_entry()
    }











    /// The active pane's filter field, its funnel toggle, and the results that
    /// replace its listing while the field has text.
    pub(in crate::ui) fn active_filter(
        &self,
    ) -> Option<(
        gtk::Entry,
        gtk::ToggleButton,
        super::inline_search::InlineSearch,
    )> {
        self.filter_at(self.browser.active_depth()?)
    }

    pub(in crate::ui) fn filter_at(
        &self,
        depth: usize,
    ) -> Option<(
        gtk::Entry,
        gtk::ToggleButton,
        super::inline_search::InlineSearch,
    )> {
        let pane = self.panes_at(depth).into_iter().next()?;
        Some((
            pane.filter_entry.clone()?,
            pane.filter_button.clone()?,
            pane.search.clone(),
        ))
    }






    pub fn cancel_pending_restore(&mut self) {
        self.pane_navigation.borrow_mut().cancel();
    }























    fn visible_panes(&self) -> Vec<&Pane> {
        match self.mode {
            BrowserMode::Columns => Vec::new(),
            BrowserMode::Icons => self.icons_panes.iter().collect(),
            BrowserMode::List => self.list_pane.iter().collect(),
        }
    }


    pub fn focused_position(&self) -> Option<(usize, usize)> {
        let focused = self.stack.root()?.focus()?;
        for pane in self.visible_panes() {
            let sections = pane.item_sections();
            let position = sections.iter().find_map(|section| {
                section.bound_items.borrow().iter().find_map(|bound| {
                    let widget = bound.widget.upgrade()?;
                    if widget != focused
                        && !widget.is_ancestor(&focused)
                        && !focused.is_ancestor(&widget)
                    {
                        return None;
                    }
                    source_position_for_view(
                        &pane.source_index,
                        Some(&section.view_model),
                        bound.item.upgrade()?.position(),
                    )
                })
            });
            if let Some(position) = position {
                return Some((pane.depth, position));
            }
        }
        None
    }



    pub fn visual_order(&self, depth: usize) -> Vec<usize> {
        let Some(pane) = self
            .visible_panes()
            .into_iter()
            .find(|pane| pane.depth == depth)
        else {
            return Vec::new();
        };
        pane.item_sections()
            .into_iter()
            .flat_map(|section| {
                (0..section.view_model.n_items())
                    .filter_map(|position| {
                        source_position_for_view(
                            &pane.source_index,
                            Some(&section.view_model),
                            position,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }





    pub(in crate::ui) fn refresh_name_highlights(&self, find: Option<&str>) {
        for pane in self.all_panes() {
            for section in pane.item_sections() {
                for bound in section.bound_items.borrow().iter() {
                    if let Some(label) = bound.rename_label.upgrade() {
                        super::browser::find::highlight_listing_name(
                            &label,
                            find,
                            &pane.filter_query.borrow(),
                        );
                    }
                }
            }
            pane.search.refresh_result_highlights(find);
        }
    }

    pub fn view_position_in(&self, view: &gtk::Widget, depth: usize, source: usize) -> Option<u32> {
        let panes = self.visible_panes();
        let pane = panes.iter().find(|pane| pane.depth == depth)?;
        pane.item_sections().into_iter().find_map(|section| {
            if section.view == *view {
                section.source_to_view(&pane.model, source)
            } else {
                None
            }
        })
    }



    fn all_panes(&self) -> Vec<&Pane> {
        self.icons_panes
            .iter()
            .chain(self.list_pane.as_ref())
            .collect()
    }




    fn panes_at(&self, depth: usize) -> Vec<&Pane> {
        match self.mode {
            BrowserMode::Columns => Vec::new(),
            BrowserMode::Icons => self
                .icons_panes
                .iter()
                .find(|pane| pane.depth == depth)
                .into_iter()
                .collect(),
            BrowserMode::List => self
                .list_pane
                .as_ref()
                .filter(|pane| pane.depth == depth)
                .into_iter()
                .collect(),
        }
    }






}












#[derive(Clone)]
struct ModeClickOptions {
    previews: Rc<Cell<bool>>,
    activation: Rc<Cell<ClickActivation>>,
    multiple_selection: Rc<Cell<bool>>,
}







/// Shared wiring every icons view in a pane needs, so a pane that groups entries by
/// type can build one view per group without threading a dozen arguments through.
struct IconsContext {
    browser: Rc<Browser>,
    depth: usize,
    click: ModeClickOptions,
    transfer: TransferHandlerSlot,
    marks: Rc<RefCell<ClipboardMarks>>,
    state: Option<Weak<super::browser::ViewState>>,
    thumbnail_size: Rc<Cell<i32>>,
    source_index: SourceIndexMap,
    sections: Weak<RefCell<Vec<PaneSection>>>,
    density: Cell<BrowserDensity>,
    scrolling: Rc<Cell<bool>>,
    filter_query: Rc<RefCell<String>>,
}



















/// Spacing for a Custom slider position, extrapolated linearly through the static
/// Compact and Airy rules in `style.css` so the slider has no jump at either preset.
pub(crate) fn browser_density_custom_css(px: u8) -> String {
    let step = (f64::from(px) - f64::from(BrowserDensity::COMPACT_PX))
        / f64::from(BrowserDensity::AIRY_PX - BrowserDensity::COMPACT_PX);
    let between = |compact: f64, airy: f64| (compact + (airy - compact) * step).max(0.0).round();
    let row_margin = px;
    let row_content_margin = px.min(4);
    let row_min_height = between(26.0, 42.0);
    let grid_padding = between(0.0, 5.0);
    let child_margin = between(1.0, 3.0);
    let card_vertical = between(2.0, 5.0);
    let card_horizontal = between(4.0, 7.0);
    format!(
        ".density-custom .file-row, .mode-list.density-custom .list-row {{ min-height: {row_min_height}px; }}\n\
         .density-custom .file-row > *, .mode-list.density-custom .list-row > * {{ margin-top: {row_margin}px; margin-bottom: {row_margin}px; }}\n\
         .density-custom .file-row > .file-row-content {{ margin-top: {row_content_margin}px; margin-bottom: {row_content_margin}px; }}\n\
         .mode-icons.density-custom .file-icons {{ padding: {grid_padding}px; }}\n\
         .mode-icons.density-custom .file-icons > child {{ margin: {child_margin}px; }}\n\
         .mode-icons.density-custom .icons-card {{ padding: {card_vertical}px {card_horizontal}px; }}\n"
    )
}






























fn source_position_for_view(
    map: &SourceIndexMap,
    view: Option<&gio::ListModel>,
    position: u32,
) -> Option<usize> {
    let Some(view) = view else {
        return Some(position as usize);
    };
    map.of_view_position(view, position)
}


pub(super) fn icon_details_need_fill(entry: &FileEntry) -> bool {
    let path = Path::new(&entry.native_name);
    (entry.is_directory() && entry.child_count == MetadataValue::Unknown)
        || (!entry.is_directory()
            && ((entry.image_dimensions == MetadataValue::Unknown
                && crate::services::is_image_path(path))
                || (entry.duration_seconds == MetadataValue::Unknown
                    && crate::services::is_media_path(path))))
}














































/// Groups a pane's rows by file type. GTK's stable sort preserves the pane's
/// filename ordering within each type.
#[derive(Clone)]
struct TypeGroupSorter {
    sorter: gtk::CustomSorter,
    /// The preferences the last comparison read, so a re-sort only follows a
    /// preference change GTK has not already sorted spliced rows under.
    compared_with: Rc<Cell<Option<crate::model::ViewPreferences>>>,
}

impl TypeGroupSorter {

}






fn selected_source_positions(source_index: &SourceIndexMap, section: &PaneSection) -> Vec<usize> {
    let selected = section.selection.selection();
    let positions = gtk::BitsetIter::init_first(&selected)
        .into_iter()
        .flat_map(|(rest, first)| std::iter::once(first).chain(rest));
    section
        .view_index
        .sources_of(source_index, &section.view_model, positions)
        .into_iter()
        .flatten()
        .collect()
}





