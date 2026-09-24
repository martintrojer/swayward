swayward inherits libinput pointer and touch handling. A vertical four-finger
swipe opens and closes the workspace overview. A `toggle-overview` bind provides
keyboard access. Configure the overview's appearance in [miscellaneous
configuration](./Configuration:-Miscellaneous.md).
Swayward does not retain niri's horizontal viewport gesture because the nested
tree has no scrolling viewport.

Configure device-level scrolling, tapping, acceleration, and calibration in the
[input section](./Configuration:-Input.md).

Window gestures use the modifier selected by `input.mod-key`:

- Hold <kbd>Mod</kbd> and drag with the left mouse button to move a window.
- Hold <kbd>Mod</kbd> and drag with the right mouse button to resize a window.
- Right-click during a move to switch the target between the tiled tree and the
  floating layer.

The same interactive move operation is available to touch and tablet input.
The insert hint in [layout configuration](./Configuration:-Layout.md#insert-hint)
shows where a tiled window will enter the tree.
