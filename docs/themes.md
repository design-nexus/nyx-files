# Themes

Files follows the Nyx theme. Its interface is styled by nyx-theme, the same
stylesheets every Nyx app uses, and the theme, accent, fonts and text size are
chosen in Nyx Settings rather than in Files. When the suite theme changes, open
windows, menus, dialogs, icons and previews follow it without a restart.

Files draws a few things with the theme's colors itself: the code preview's
syntax scheme, rendered documents, the audio player and the 3D model palette.
These take the active theme's palette and, when the theme has one, its syntax
colors (`keyword`, `string`, `constant`, `type` and `preprocessor`); missing
syntax colors are derived from the accent and text colors. Dim text is lifted
until it reads at 4.5:1 on the background and 3:1 on surfaces, stopping at the
first step that passes so it stays dimmer than body text.

Interface icons come from the desktop icon theme as symbolic icons, so they
take the theme's text and accent colors.

## User stylesheet

To restyle Files itself, create an optional stylesheet at:

```text
~/.config/nyx-files/style.css
```

It is loaded above every stylesheet Files and nyx-theme ship, so its rules win.
The theme's named colors, such as `@theme_bg`, `@theme_surface`, `@theme_text`,
`@theme_dim_text`, `@theme_accent` and `@theme_border`, are available and
follow theme changes:

```css
/* A tinted sidebar */
.sidebar {
    background: alpha(@theme_surface, 0.6);
}
```

Files re-reads the file when it changes, so edits apply to open windows
without a restart; deleting it restores the default appearance. Rules GTK
cannot parse are skipped and logged as warnings. Class names are not a stable
interface and may change between releases.
