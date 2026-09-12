# Frequently asked questions

## How do I disable client-side decorations?

Enable `prefer-no-csd` at the top level of the config, then restart affected
applications. swayward asks clients to omit their decorations and marks tiled
windows as tiled.

## Why are transparent windows tinted? Why is the border/focus ring showing up through semitransparent windows?

By default, the focus ring and border are solid rectangles behind windows.
Enable `prefer-no-csd`, or set `draw-border-with-background false` in a window
rule. See [Window rules](./Configuration:-Window-Rules.md#draw-border-with-background).

## How do I round window corners?

```kdl
window-rule {
    geometry-corner-radius 12
    clip-to-geometry true
}
```

## How do I run X11 applications?

Install xwayland-satellite 0.7 or newer. swayward creates the X11 socket and
starts it on demand. See [Xwayland](./Xwayland.md).

## How do I recover from a failed screen locker?

Start another locker on swayward's Wayland display from a different TTY. You
can also configure a locker bind with `allow-when-locked=true`. The red
background means that the session remains locked.

## How do I select output profiles?

Use [Kanshi](https://gitlab.freedesktop.org/emersion/kanshi) to apply output
configurations based on the connected monitors.

## Can I use sway tools?

Yes. swayward exports `SWAYSOCK` and implements sway's IPC protocol. Handler
coverage is still incomplete, so unsupported requests return explicit errors.
See [IPC](./IPC.md).

## Why doesn't swayward integrate Xwayland directly?

xwayland-satellite contains the X11 window-manager integration and presents X11
clients as regular Wayland windows. Keeping that complexity out of swayward
reduces the compositor's maintenance and crash surface.
