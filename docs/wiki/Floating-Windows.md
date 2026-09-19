# Floating windows

Floating windows appear above the tiled container tree. Each workspace has its
own floating layer.

Dialogs and fixed-size windows float automatically. Use `open-floating` in a
window rule to override that decision:

```kdl
window-rule {
    match app-id="firefox$" title="^Picture-in-Picture$"
    open-floating true
}
```

The `toggle-window-floating` action moves the focused window between the tree
and the floating layer. `switch-focus-between-floating-and-tiling` changes
which layer has focus. While a floating window has focus, directional move and
resize operations act on that window.

Set `default-floating-position` in a [window
rule](./Configuration:-Window-Rules.md#default-floating-position) to choose an
initial position.
