## Install

There is no COPR repository and no AUR package. Packages are published on the
[releases page](https://github.com/martintrojer/swayward/releases) instead.
Each release carries these x86_64 downloads, where `VERSION` is the release
version:

| File | For |
|---|---|
| `swayward_VERSION_amd64.deb` | Ubuntu 24.04 |
| `swayward-VERSION-fedora43-x86_64.rpm` | Fedora 43 |
| `swayward-VERSION-fedora44-x86_64.rpm` | Fedora 44 |
| `swayward-VERSION-arch-x86_64.pkg.tar.zst` | Arch |
| `swayward-VERSION-ubuntu24.04-x86_64.tar.gz` | prebuilt binaries |

Fedora gets one RPM per supported release. Use the file matching your Fedora
version, which `cat /etc/fedora-release` reports.

Download the file for your distribution, then install it:

```sh
sudo apt install ./swayward_VERSION_amd64.deb            # Ubuntu 24.04
sudo dnf install ./swayward-VERSION-fedora44-x86_64.rpm  # Fedora 44
sudo pacman -U ./swayward-VERSION-arch-x86_64.pkg.tar.zst
```

The binary tarball ships a `.sha256` file. Verify it before unpacking:

```sh
sha256sum -c swayward-VERSION-ubuntu24.04-x86_64.tar.gz.sha256
tar xzf swayward-VERSION-ubuntu24.04-x86_64.tar.gz
```

The tarball is built on Ubuntu 24.04 and needs GLIBC 2.39 or newer and
`libdisplay-info.so.1`. It is not a generic Linux build. A
`-vendored-dependencies.tar.xz` archive is also attached, for building offline
rather than installing.

With Nix, install the current repository version on any supported Linux system:

```sh
nix profile install github:martintrojer/swayward
```

On NixOS, you can also consume `github:martintrojer/swayward` as a flake input.
The flake exports the `swayward` and `swayward-debug` packages, an overlay, and
a development shell.

Without Nix, build from source. See [Build swayward](https://github.com/martintrojer/swayward/blob/main/docs/BUILDING.md).

The recipes behind those packages live in the repository and build in clean
containers, but no one outside the project has installed from them yet.

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
