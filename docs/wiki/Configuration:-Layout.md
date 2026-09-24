swayward arranges tiled windows in a fully nested i3-style container tree. Each
container has one of four layouts:

- `splith` places its children from left to right.
- `splitv` places its children from top to bottom.
- `tabbed` shows one child at a time and draws a title strip.
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

### `default-orientation`

`default-orientation` selects the root split for each new workspace. Use
`"horizontal"`, `"vertical"`, or `"auto"`. The default `"auto"` uses a
vertical root on portrait outputs and a horizontal root otherwise.

```kdl
layout {
    default-orientation "auto"
}
```

### `workspace-layout`

`workspace-layout` wraps each new top-level window in the selected layout. Use
`"default"` for an ordinary split, `"tabbed"` for tabs, or `"stacking"` for a
vertical title list.

```kdl
layout {
    workspace-layout "tabbed"
}
```

## Visual options

The inherited `layout` options configure the tree's presentation:

```kdl
layout {
    gaps 0

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

### `gaps` and `outer-gaps`

`gaps` sets the space between tiled windows in logical pixels. It defaults to
zero, matching sway. Fractional values are rounded to physical pixels for each
output.

`outer-gaps` sets each workspace edge independently. Each edge is added to
`gaps`; negative values can reduce the effective outer space to zero. Swayward
shrinks excessive values proportionally to leave at least a 100×60 workspace
area.

```kdl
layout {
    gaps 16
    outer-gaps {
        left 8
        right 8
        top 0
        bottom 0
    }
}
```

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

### `default-border` and `default-floating-border`

These settings set the initial sway border style for future tiled and floating
windows. Use `"normal"`, `"pixel"`, or `"none"`; add `width=` to set an
explicit width in logical pixels. Changing either setting does not restyle
windows that already exist.

```kdl
layout {
    default-border "pixel" width=2
    default-floating-border "normal" width=3
}
```

### `titlebar`

`titlebar` styles server-side window titlebars and tabbed or stacked title
strips. Set `font`, `horizontal-padding`, `vertical-padding`, and
`border-thickness`. Set `pango-markup true` to render title formats as Pango
markup. The border thickness cannot exceed either padding value.

Each state block accepts `border-color`, `background-color`, and `text-color`:
`focused`, `focused-inactive`, `focused-tab-title`, `unfocused`, and `urgent`.

```kdl
layout {
    titlebar {
        font "monospace 10"
        pango-markup false
        horizontal-padding 5
        vertical-padding 8
        border-thickness 4

        focused {
            border-color "#ffc87f"
            background-color "#4775a3"
            text-color "#fff"
        }
    }
}
```

### `draw-uncovered-top-border`

In a tabbed or stacked container, inactive titles can leave part of the focused
window's top outline uncovered. `draw-uncovered-top-border true` fills those
spans. Set it to `false` for sway's rendering, which leaves them empty.

### `shadow`

`shadow` controls window shadows. `softness`, `spread`, and `offset` use logical
pixels. `color` and `inactive-color` accept CSS colours. Shadows are disabled by
default because they require additional rendering work.

### `tab-indicator`

The optional tab indicator supplements the title strip for a tabbed container.
It is disabled by default. Set `on` to enable it, then configure its position,
width, length, gaps, corner radius, and colours here. See [Tabbed
containers](./Tabs.md).

### `insert-hint`

The insert hint shows where an interactively moved window will enter the tree.
Set `off` to disable it. Use `color` or `gradient` to change its appearance.

### `struts`

Struts reserve space at each edge of the workspace. They act like configurable
outer gaps in addition to layer-shell exclusive zones. Values use logical
pixels and may be negative.

### `floating-minimum-size` and `floating-maximum-size`

These settings constrain the default size of a tiled window when it first
becomes floating or enters the scratchpad. Each takes width and height in
logical pixels. For either dimension, `-1` removes the limit and `0` selects
sway's automatic limit. The defaults are `floating-minimum-size 75 50` and
`floating-maximum-size 0 0`.

```kdl
layout {
    floating-minimum-size 100 80
    floating-maximum-size 1600 900
}
```

### `background-color`

`background-color` sets the colour behind windows when no wallpaper surface
covers the workspace.

## Pop-ups during fullscreen

`popup-during-fullscreen` is a top-level setting that controls new windows whose
parent is fullscreen:

- `"smart"` keeps the parent relationship and permits normal focus behavior.
- `"ignore"` drops the parent relationship and does not focus the new window.
- `"leave_fullscreen"` exits the parent's fullscreen state and does not focus
  the new window.

```kdl
popup-during-fullscreen "smart"
```
