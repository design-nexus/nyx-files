# Preview panel layout

This page describes how the preview drawer shares a tab with the file views.
Changes to the preview split or its width rules must keep it accurate. Verify
geometry manually with captures; do not add layout assertions.

## Tab split

Each tab (`src/interface/tab.rs`) is a vertical box: a horizontal
`gtk::Paned` above the shortcut footer.

```
tab container (gtk::Box, vertical)
├─ split (gtk::Paned)
│  ├─ start: view stack: Columns, Icons, List, filter results, status
│  └─ end:   preview slot (gtk::Box) › revealer › preview pane
└─ shortcut footer
```

The sidebar sits beside the tab pages in the window, outside the split. Only the
start child resizes when the window changes size, so the preview keeps its width
and the file views absorb the difference. The file chooser uses the same tab and
the same drawer code in `src/ui/preview.rs` and `src/ui/preview/layout.rs`
(`PreviewDrawer::attach_split_only`).

`sync_split` runs once per frame while the drawer is enabled or reserving space.
It owns the slot's visibility, minimum width and divider position; nothing else
positions the divider except the user's drag and the reveal animation, which
slides the drawer open or closed in 260 ms (`TRANSITION`), or at once with
reduced motion.

## Width rules

| Rule | Value |
| --- | --- |
| Preview minimum | 240 px (`MIN_WIDTH`) |
| Narrow-window hide threshold | 240 px (`MIN_SPLIT_PREVIEW_WIDTH`) |
| Automatic maximum | 3000 px (`MAX_WIDTH`) |
| Image, 3D model, video, PDF and rendered document content maximum | 1280 px (`media_layout::MAX_CONTENT_WIDTH`), centered in wider previews; PDF zoom scales from it |

- **Automatic width** is 90% of half the content width, clamped to the minimum
  and maximum.
- **Manual width** comes from dragging the divider or moving it with the
  keyboard. It is window-local and session-local, and never goes below the hide
  threshold.
- **Narrow windows**: the file view has priority. The preview shrinks to its
  minimum, then hides when less than the threshold would remain. A hidden
  preview pauses media and defers loading; widening the window restores it with
  the same selection and width.

## Opening and closing

| Input | Effect on the drawer |
| --- | --- |
| **Space** on a file, **i** in 10xer mode, Appearance → Preview panel | Toggles the drawer for the selected file. |
| **l** / **Right** on a file in 10xer mode | Opens the drawer and gives it the keys. |
| **Esc** in the drawer, the close button | Closes it. |
| **Shift+Tab** in the drawer | Returns the keys to the listing; the drawer stays open. |
| Single click with **Single-click file previews** on | Previews the clicked file. |

While the drawer has focus its keys follow the content: documents scroll,
archives step through their entries, players take the media keys
(`src/interface/preview_keys.rs`).

## Manual check

1. In List view, select a file and press **Space**: the drawer slides open
   beside the list at its automatic width.
2. Drag the divider narrower, select another file: the width stays.
3. Narrow the window until the preview hides, then widen it: the same file
   returns at the previous width.
4. Press **Esc** in the drawer: it closes and the list keeps the selection.
