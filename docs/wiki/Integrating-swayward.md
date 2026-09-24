Install swayward as a standalone Wayland session. See [Packaging
swayward](./Packaging-swayward.md) for file locations and recommended runtime
dependencies.

## Configuration

swayward reads `$XDG_CONFIG_HOME/swayward/config.kdl` or
`~/.config/swayward/config.kdl`, then `/etc/swayward/config.kdl`. Distributions
can provide `/etc/swayward/config.kdl` as their default. Keep local defaults in
sync with changes to `resources/default-config.kdl`.

Set `SWAYWARD_CONFIG` to override the path.

## Session services

`swayward.service` starts `graphical-session.target` and
`xdg-desktop-autostart.target`. Desktop components can use normal XDG autostart
files or user units wanted by `graphical-session.target`.

A usable session normally includes:

- a notification daemon;
- `xdg-desktop-portal-gnome` as the preferred backend, `xdg-desktop-portal-gtk` as its fallback, and Nautilus for the GNOME 47 or later FileChooser implementation;
- an authentication agent;
- a panel, launcher, wallpaper tool, and screen locker;
- xwayland-satellite for X11 applications.

See [Important software](./Important-Software.md) and [Example systemd
setup](./Example-systemd-Setup.md).

## Keyboard layout

Unless the KDL config specifies an XKB layout, swayward reads the system layout
from `org.freedesktop.locale1`. Installers should set the layout through
systemd-localed.

## Accessibility

A full session exposes the D-Bus and AccessKit interfaces used by Orca. See
[Accessibility](./Accessibility.md).

---

*This page is adapted from the niri documentation.*
