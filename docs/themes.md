# Theme files

Magnitide themes are a **palette** (named colours) plus optional **role** overrides
(what the UI paints with). Add-on themes are JSON files in the app's themes folder:
Settings → Appearance → Theme add-ons → **Add Theme File…** copies one in, and
**Open Themes Folder** shows where they live:

| OS      | Folder                                                   |
| ------- | -------------------------------------------------------- |
| macOS   | `~/Library/Application Support/com.magnitide.app/themes` |
| Windows | `%APPDATA%\com.magnitide.app\themes`                     |
| Linux   | `~/.config/com.magnitide.app/themes`                     |

(The folder is the app's config folder plus `themes`; **Open Themes Folder** is the
reliable way to find it.) After editing a file, press **Reload**. Files that can't be used are
listed there with the reason.

A theme file is data, not CSS: it can recolour the app but not restyle it, hide parts
of it, or load anything. Every colour is checked against the grammar below.

## A theme file

```json
{
  "format": 1,
  "id": "rose-pine",
  "name": "Rosé Pine",
  "author": "you",
  "appearance": "dark",
  "palette": {
    "crust": "#191724",
    "mantle": "#1f1d2e",
    "base": "#191724",
    "surface-0": "#26233a",
    "surface-1": "#403d52",
    "surface-2": "#524f67",
    "overlay-0": "#6e6a86",
    "overlay-1": "#6e6a86",
    "overlay-2": "#908caa",
    "subtext-0": "#908caa",
    "subtext-1": "text 80% subtext-0",
    "text": "#e0def4",
    "highlight-low": "#21202e",
    "love": "#eb6f92",
    "gold": "#f6c177",
    "rose": "#ebbcba",
    "pine": "#31748f",
    "foam": "#9ccfd8",
    "iris": "#c4a7e7"
  },
  "roles": {
    "accent": "iris",
    "link": "foam",
    "danger": "love",
    "warning": "gold",
    "success": "foam",
    "info": "foam",
    "star": "gold",
    "hover": "highlight-low",
    "text-selection": "surface-1"
  },
  "accents": [
    "love",
    "gold",
    { "name": "rose", "label": "Rosé" },
    "pine",
    "foam",
    "iris"
  ]
}
```

(An illustration using Rosé Pine's published palette; check the values against
[rosepinetheme.com](https://rosepinetheme.com) before relying on them.)

| Field        | Required | What it is                                                                                                                                  |
| ------------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `format`     | yes      | `1`. Newer formats will say so; older files keep working.                                                                                   |
| `id`         | yes      | 1–48 lowercase letters, digits and hyphens. Unique across your theme files.                                                                 |
| `name`       | yes      | Shown in the theme menu.                                                                                                                    |
| `author`     | no       | Shown in Settings.                                                                                                                          |
| `appearance` | yes      | `"light"` or `"dark"`: picks the defaults for toasts, the dialog backdrop and shadows.                                                      |
| `palette`    | yes      | Named colours. Needs at least `base` and `text`.                                                                                            |
| `roles`      | no       | Overrides for the roles below.                                                                                                              |
| `accents`    | no       | Palette colours the accent picker offers, as names or `{ "name", "label" }`. Default: whichever of Catppuccin's 14 accents the palette has. |

## The palette

Name colours after [Catppuccin's palette](https://github.com/catppuccin/catppuccin/blob/main/docs/style-guide.md),
because that's what the roles' defaults read:

- the neutral steps, darkest to lightest in a dark theme (reversed in a light one):
  `crust`, `mantle`, `base`, `surface-0`, `surface-1`, `surface-2`, `overlay-0`,
  `overlay-1`, `overlay-2`, `subtext-0`, `subtext-1`, `text`;
- the accents: `rosewater`, `flamingo`, `pink`, `mauve`, `red`, `maroon`, `peach`,
  `yellow`, `green`, `teal`, `sky`, `sapphire`, `blue`, `lavender`.

Neutral steps you leave out are mixed from `base` and `text`, so a short palette still
works. You can add colours of your own (`love`, `foam`…) to use in roles and accents.
Names are lowercase letters, digits and hyphens; role names and a few the app uses
internally (`hue`, `card-shadow`, `lift-shadow`, `scrim`, and anything starting `tw-`,
`color-` or `shadow-`) aren't allowed.

**Porting a Catppuccin port:** a Catppuccin-style `palette.json` (flavours with `dark`
and `colors`, each colour with a `hex`) can be added as it is; each flavour becomes a
theme, set up like the built-in Catppuccin ones.

**Porting from Dracula's spec:** see `crates/app/ui/src/themes/dracula.css` for how its
colours map onto these names.

## Colour values

Everywhere a colour goes (palette or roles), a value is one of:

| Value                          | Meaning                                                                             |
| ------------------------------ | ----------------------------------------------------------------------------------- |
| `#rgb`, `#rrggbb`, `#rrggbbaa` | A hex colour.                                                                       |
| `surface-2`                    | Another colour in this theme (palette or role), or `transparent`, `black`, `white`. |
| `overlay-2 25%`                | That colour at 25%, over transparent.                                               |
| `text 62% overlay-1`           | 62% of the first mixed with the second.                                             |

Nothing else is accepted.

## Roles

What the UI paints with. Each has a default from the palette (in
`crates/app/ui/src/themes/roles.css`), so override only what your theme needs.

| Role                                   | Used for                          | Default                                      |
| -------------------------------------- | --------------------------------- | -------------------------------------------- |
| `bg`                                   | Main background                   | `base`                                       |
| `panel`                                | Side panes (the details panel)    | `mantle`                                     |
| `popover`                              | Menus and dialogs                 | `mantle`                                     |
| `field`                                | Text inputs                       | `base`                                       |
| `element`, `element-hover`             | Chips, buttons, tracks            | `surface-0`, `surface-1`                     |
| `fg`, `fg-muted`, `fg-faint`           | Body text, labels, subtle text    | `text`, `subtext-0`, `overlay-1`             |
| `link`                                 | Links                             | `blue`                                       |
| `accent`, `on-accent`                  | The accent, and text on it        | `blue`, `base`                               |
| `focus`                                | Focus rings                       | `accent`                                     |
| `caret`                                | The text cursor                   | `fg`                                         |
| `hover`                                | Hovered rows and buttons          | `text 10%`                                   |
| `row-stripe`                           | Alternate table rows              | `text 8%`                                    |
| `selection`, `on-selection`            | Selected rows, and text on them   | `accent`, `on-accent`                        |
| `text-selection`                       | Selected text                     | `overlay-2 25%`                              |
| `highlight`                            | The active item in a menu or list | `accent 15%`                                 |
| `success`, `warning`, `danger`, `info` | Status                            | `green`, `yellow`, `red`, `teal`             |
| `on-status`                            | Text on status fills              | `base`                                       |
| `star`                                 | Ratings                           | `yellow`                                     |
| `border`                               | Hairlines                         | `overlay-0 35%`                              |
| `inverse`, `on-inverse`                | Toasts and the selection bar      | dark: `crust`, `text`; light: `text`, `base` |
| `scrim`                                | Behind dialogs                    | dark: black 38%; light: `text 22%`           |
| `shadow`                               | Shadow colour                     | dark: black 75%; light: black 20%            |

Corner radii (`--control-radius` and friends in `roles.css`) are part of the app's shape,
not its colours, so theme files don't set them (yet).
