// SPDX-License-Identifier: MIT

use super::columns::{ColumnSpan};
use super::*;

impl ViewState {
    pub(super) fn column_span(&self, depth: usize) -> Option<ColumnSpan> {
        let columns = self.columns.borrow();
        let column = columns.get(depth)?;
        let left = columns[..depth]
            .iter()
            .map(column_width)
            .fold(0, i32::saturating_add);
        Some(ColumnSpan {
            left: f64::from(left),
            right: f64::from(left.saturating_add(column_width(column))),
            trailing: f64::from(self.columns_width(depth.saturating_add(1)..)),
        })
    }

    /// The focused column, or the one a pending click is opening, so the reserved
    /// slot does not resize between the press and the open.
    fn focused_depth(&self) -> Option<usize> {
        self.pointer_opening_depth()
            .map(|parent| parent.saturating_add(1))
            .or_else(|| self.browser.active_depth())
    }

    fn focused_column_span(&self) -> Option<ColumnSpan> {
        let count = self.columns.borrow().len();
        let depth = self
            .focused_depth()
            .filter(|depth| *depth < count)
            .or_else(|| count.checked_sub(1))?;
        self.column_span(depth)
    }

    fn navigated_len(&self) -> usize {
        let count = self.columns.borrow().len();
        // Columns beyond a folder a click is opening are about to close, so they
        // borrow no preview space in the meantime.
        if self.pointer_opening_depth().is_some() {
            return count;
        }
        self.browser
            .active_depth()
            .map_or(count, |depth| depth.saturating_add(1).min(count))
    }

    fn columns_width(
        &self,
        range: impl std::slice::SliceIndex<[ColumnView], Output = [ColumnView]>,
    ) -> i32 {
        self.columns.borrow().get(range).map_or(0, |columns| {
            columns
                .iter()
                .map(column_width)
                .fold(0, i32::saturating_add)
        })
    }
}

fn column_width(column: &ColumnView) -> i32 {
    column
        .shell
        .width()
        .max(column.shell.width_request())
        .max(COLUMN_WIDTH)
}

impl BrowserView {
    pub(in crate::ui) fn pointer_opens(&self, depth: usize, location: &Location) -> bool {
        self.state.pointer_opens(depth, location)
    }

    pub(in crate::ui) fn is_resizing_columns(&self) -> bool {
        self.state.column_resizing.get()
    }

    /// Bounds Miller column widths. The first limit, and the first after a text-size
    /// change, also narrow wider open columns; a shrinking window does not.
    pub(in crate::ui) fn set_column_width_limit(&self, limit: i32) {
        self.state.column_width_limit.set(Some(limit));
        if !self.state.clamp_to_next_column_limit.replace(false) {
            return;
        }
        let limit = limit.max(super::columns::column_floor());
        for column in self.state.columns.borrow().iter() {
            if column.shell.width_request() > limit {
                // Stops a running width animation from restoring the oversized width.
                column
                    .animation_generation
                    .set(column.animation_generation.get().saturating_add(1));
                column.shell.set_size_request(limit, -1);
            }
        }
    }

    pub(in crate::ui) fn preview_occupied_width(&self, available: i32) -> i32 {
        if self.view_mode() != BrowserMode::Columns {
            return single_pane_preview_reservation(available);
        }
        self.state
            .columns
            .borrow()
            .iter()
            .map(column_width)
            .fold(0, i32::saturating_add)
    }

    pub(in crate::ui) fn preview_navigated_width(&self, available: i32) -> i32 {
        if self.view_mode() != BrowserMode::Columns {
            return single_pane_preview_reservation(available);
        }
        self.state.columns_width(..self.state.navigated_len())
    }

    pub(in crate::ui) fn preview_trailing_width(&self) -> i32 {
        if self.view_mode() != BrowserMode::Columns {
            return 0;
        }
        self.state.columns_width(self.state.navigated_len()..)
    }

    pub(in crate::ui) fn preview_navigation_width(&self) -> i32 {
        self.state
            .focused_column_span()
            .map_or(COLUMN_WIDTH, |span| span.width() as i32)
    }

    pub(in crate::ui) fn preview_standard_navigation_width(&self) -> i32 {
        self.state
            .focused_column_span()
            .map_or(COLUMN_WIDTH, |span| {
                span.width().min(f64::from(COLUMN_WIDTH)) as i32
            })
    }


    pub(in crate::ui) fn preserve_columns_for_viewport(&self, available: i32) {
        if self.view_mode() != BrowserMode::Columns {
            return;
        }
        let offset = self.state.scroller.hadjustment().value();
        let occupied = self.preview_occupied_width(available);
        let gap = if offset > 0.0 {
            (offset + f64::from(available - occupied)).ceil().max(0.0) as i32
        } else {
            0
        };
        self.state.columns_widget.set_margin_end(gap);
        self.state.horizontal_scroll_generation.set(
            self.state
                .horizontal_scroll_generation
                .get()
                .saturating_add(1),
        );
    }

    pub(in crate::ui) fn clear_preview_scroll_space(&self) {
        self.state.columns_widget.set_margin_end(0);
    }

    // Shrinking the preserved margin re-clamps the scroll offset each frame,
    // so columns slide into the reclaimed viewport instead of snapping.

}
