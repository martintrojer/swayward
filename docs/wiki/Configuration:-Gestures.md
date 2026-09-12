# Gesture configuration

swayward inherits libinput pointer and touch handling, but it does not retain
niri's horizontal view scrolling or overview gestures. Configure device-level
scrolling, tapping, acceleration, and calibration in the [input
section](./Configuration:-Input.md).

Window gestures use the modifier selected by `input.mod-key`:

- Hold <kbd>Mod</kbd> and drag with the left mouse button to move a window.
- Hold <kbd>Mod</kbd> and drag with the right mouse button to resize a window.
- Right-click during a move to switch the target between the tiled tree and the
  floating layer.

The same interactive move operation is available to touch and tablet input.
The insert hint in [layout configuration](./Configuration:-Layout.md#insert-hint)
shows where a tiled window will enter the tree.
