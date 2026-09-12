# Tabbed containers

A tabbed container gives its full content area to one child at a time. The
other children remain in the tree and keep their layout state.

Use the layout commands that select `tabbed`, `split-h`, `split-v`, or `stacked`
to change the focused container's layout. Focus and move commands operate on
the same nested tree in every layout.

## Tab indicator

Tabbed containers draw an indicator for their children. Click an item in the
indicator to focus that tab.

Configure the indicator in the [`tab-indicator` layout
section](./Configuration:-Layout.md#tab-indicator). You can change its side,
width, length, spacing, corner radius, and active, inactive, and urgent colours.
Set `hide-when-single-tab` to hide the indicator when a tabbed container has one
child.
