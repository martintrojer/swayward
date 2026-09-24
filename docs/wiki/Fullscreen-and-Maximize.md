swayward supports fullscreen, maximized, and windowed-fullscreen states.

## Maximize

Maximizing a tiled window expands it within its workspace while preserving the
container tree so that the previous arrangement can be restored. A maximized
floating window uses the output's available area.

Use `open-maximized true` in a window rule to start a matching window
maximized.

## Fullscreen

A fullscreen window covers its output, including normal layer-shell panels and
floating windows. Overlay-layer surfaces can still appear above it. The window
remains in its workspace and returns to its prior tree position or floating
geometry when fullscreen ends.

Use `open-fullscreen true` to start a matching window fullscreen. Set it to
`false` to reject an application's initial fullscreen request.

## Windowed fullscreen

Windowed fullscreen tells the application that it is fullscreen without making
its tile cover the output. This is useful for browser presentations that must
hide their UI while remaining visible beside other windows.

The `toggle-windowed-fullscreen` action controls this state.
