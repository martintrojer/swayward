# Gestures

swayward supports interactive move and resize gestures for windows. It does not
include niri's horizontal viewport, overview, or edge-scroll gestures because
those concepts do not exist in the nested tree.

## Mouse

Hold <kbd>Mod</kbd> and drag with the left mouse button to move a window. Hold
<kbd>Mod</kbd> and drag with the right mouse button to resize it. Right-click
while moving to switch between the tiled tree and the floating layer.

## Touch and tablet

Hold <kbd>Mod</kbd> and drag a window with a finger or tablet pen to perform an
interactive move. The [insert hint](./Configuration:-Layout.md#insert-hint)
shows the destination in the tiled tree.

Configure device behaviour in [Input](./Configuration:-Input.md).
