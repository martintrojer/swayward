Swayward keeps sway's nested-tree behavior and IPC contract where possible, but
it is not a drop-in replacement for every sway subsystem.

Check these differences before moving a session:

- **Configuration uses KDL.** Run `swayward-sway-to-kdl` and review every
  manual-attention item. See [Migrate a sway config](https://github.com/martintrojer/swayward/wiki/SWAY_CONFIG_MIGRATION).
- **No managed bar.** Swayward has no `bar {}` block and does not launch
  swaybar. Configure Waybar or another layer-shell bar directly.
- **X11 identity is limited.** `xwayland-satellite` presents X11 applications as
  Wayland toplevels, so separate X11 class, instance, role, and XID data do not
  reach swayward.
- **Some runtime commands are missing.** Unsupported commands fail explicitly.
  See [Sway compatibility](Sway-Compatibility.md) for the practical boundary.
- **Some i3 structures are absent because sway omits them too.** Examples
  include floating wrapper nodes, output `content` nodes, JSON layout restore,
  and the i3 `open` command.
- **Floating split containers are deferred.** Sway can float a whole nested
  container. Swayward currently floats windows only.
- **Desktop integration differs.** Swayward defaults to the GNOME portal
  backend for its window picker and dynamic cast target.

The [source-cited decision ledger](https://github.com/martintrojer/swayward/blob/main/docs/KNOWN_DEVIATIONS.md)
covers exact command behavior, workspace rules, Xwayland boundaries, and the
reasons behind each deliberate difference.
