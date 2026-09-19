# Layout configuration

swayward arranges tiled windows in a fully nested i3-style container tree. Each
container has one of four layouts:

- `split-h` places its children from left to right.
- `split-v` places its children from top to bottom.
- `tabbed` shows one child at a time and draws a tab indicator.
- `stacked` shows one child at a time in a vertical stack.

A workspace starts with a horizontal root container. Splitting a focused node
creates nested containers as needed. Removing or moving the last child out of a
container collapses the empty container.

## Size allocation

Children of split containers share the available space. Interactive resize
changes the proportions of the two children on either side of the dragged
boundary. Gaps, borders, focus rings, and struts reduce the space available to
the tree.

Tabbed and stacked containers give their full content area to the focused
child. Other children remain in the tree but are not rendered.

## Visual options

The inherited `layout` options configure the tree's presentation:

```kdl
layout {
    gaps 16

    focus-ring {
        on
        width 4
        active-color "#7fc8ff"
        inactive-color "#505050"
        urgent-color "#9b0000"
    }

    border {
        off
        width 4
        active-color "#ffc87f"
        inactive-color "#505050"
        urgent-color "#9b0000"
    }

    shadow {
        off
        softness 30
        spread 5
        offset x=0 y=5
        draw-behind-window true
        color "#00000070"
    }

    tab-indicator {
        on
        hide-when-single-tab
        gap 5
        width 4
        length total-proportion=1.0
        position "right"
        gaps-between-tabs 2
        corner-radius 8
    }

    insert-hint {
        on
        color "#ffc87f80"
    }

    struts {
        // left 64
        // right 64
        // top 64
        // bottom 64
    }

    background-color "#003300"
}
```

### `gaps`

`gaps` sets the space around tiled windows in logical pixels. Fractional values
are rounded to physical pixels for each output.

### `focus-ring` and `border`

The focus ring appears around the active window. Borders appear around every
window and consume layout space. Both support `width`, active, inactive, and
urgent colours, plus the gradient options documented in the default config.

Set `off` to disable either decoration. Set `on` to enable it.

Use `hide-edge-borders` to suppress tiled-window borders at workspace edges:

- `"none"` keeps all edges.
- `"vertical"` hides left and right workspace edges.
- `"horizontal"` hides top and bottom workspace edges.
- `"both"` hides all workspace edges.

Use `smart-borders` independently of `hide-edge-borders`:

- `"off"` disables smart suppression.
- `"on"` hides all borders when the workspace has one visible tiled window.
- `"no-gaps"` does the same only when `gaps` is zero.

Smart suppression only hides more edges. It does not restore an edge hidden by
`hide-edge-borders`. Floating windows keep all border edges.

### `shadow`

`shadow` controls window shadows. `softness`, `spread`, and `offset` use logical
pixels. `color` and `inactive-color` accept CSS colours. Shadows are disabled by
default because they require additional rendering work.

### `tab-indicator`

The tab indicator shows the children of a tabbed container. Configure its
position, width, length, gaps, corner radius, and colours here. See [Tabbed
containers](./Tabs.md).

### `insert-hint`

The insert hint shows where an interactively moved window will enter the tree.
Set `off` to disable it. Use `color` or `gradient` to change its appearance.

### `struts`

Struts reserve space at each edge of the workspace. They act like configurable
outer gaps in addition to layer-shell exclusive zones. Values use logical
pixels and may be negative.

### `background-color`

`background-color` sets the colour behind windows when no wallpaper surface
covers the workspace.
