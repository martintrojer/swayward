# Getting started

## Install

On Fedora:

```sh
sudo dnf copr enable martintrojer/swayward
sudo dnf install swayward
```

On Arch, install `swayward` from the AUR. On NixOS, use the flake at
`github:martintrojer/swayward`. Each release also ships an `x86_64` tarball.

To build from source, see [Build swayward](https://github.com/martintrojer/swayward/blob/main/docs/BUILDING.md).

Run the compositor in a window from an existing Wayland session while testing:

```sh
./target/release/swayward
```

For a full session, install the files listed in [Packaging
swayward](./Packaging-swayward.md), then choose swayward in your display manager.
You can also run `swayward-session` from a TTY.

> [!NOTE]
> swayward is beta. Keep another compositor or desktop session available so
> that you can recover from a startup or display failure.

## Configuration

swayward reads `~/.config/swayward/config.kdl`. Start with
`resources/default-config.kdl`, then see
the [configuration introduction](./Configuration:-Introduction.md).

The config uses KDL and retains the inherited input, output, animation, window
rule, and layer rule blocks. Layout behaviour follows an i3-style nested tree,
not scrollable tiling. See [Layout configuration](./Configuration:-Layout.md).

## IPC tools

swayward exports `SWAYSOCK` and speaks sway's IPC protocol. Use the bundled
`swaywardmsg` client or an existing i3 or sway client library. `swaymsg` works
too if you have it. See [IPC](./IPC.md) for the currently implemented request
types.

## Desktop components

For file pickers, secrets, and screen sharing, install
`xdg-desktop-portal-gnome`, `xdg-desktop-portal-gtk`, `gnome-keyring`, and
Nautilus for the GNOME 47 or later file chooser. Portals work only in a full
swayward session with `swayward-portals.conf` installed in
`/usr/share/xdg-desktop-portal/`; starting a source build directly from another
desktop does not set them up. Swayward defaults to the GNOME backend for its
window picker and dynamic cast target. `xdg-desktop-portal-wlr` is an optional,
less integrated ScreenCast and Screenshot fallback. The GNOME integration does
not support remote control or input injection. See [Important
software](./Important-Software.md).

A compositor session also needs a notification daemon, an authentication
agent, and usually a panel and launcher. X11 applications use
[xwayland-satellite](./Xwayland.md).

## Manual installation

See [Packaging swayward](./Packaging-swayward.md) for destination paths.
