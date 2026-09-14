# Getting started

swayward is under active development and does not yet publish stable packages.
Build it in the development container described in `docs/BUILDING.md`:

```sh
distrobox enter swayward-dev -- bash -lc 'cd /path/to/swayward && cargo build --release'
```

Run the compositor in a window from an existing Wayland session while testing:

```sh
./target/release/swayward
```

For a full session, install the files listed in [Packaging
swayward](./Packaging-swayward.md), then choose swayward in your display manager.
You can also run `swayward-session` from a TTY.

> [!WARNING]
> swayward is not ready to replace a stable daily-driver session. Keep another
> compositor or desktop session available so that you can recover from startup
> and display failures.

## Configuration

swayward reads `~/.config/swayward/config.kdl`. Start with
`resources/default-config.kdl`, then see
the [configuration introduction](./Configuration:-Introduction.md).

The config uses KDL and retains the inherited input, output, animation, window
rule, and layer rule blocks. Layout behaviour follows an i3-style nested tree,
not scrollable tiling. See [Layout configuration](./Configuration:-Layout.md).

## IPC tools

swayward exports `SWAYSOCK` and speaks sway's IPC protocol. Use `swaymsg` and
existing i3 or sway client libraries. See [IPC](./IPC.md) for the currently
implemented request types.

## Desktop components

A compositor session also needs a notification daemon, portals, an
authentication agent, and usually a panel and launcher. See [Important
software](./Important-Software.md). X11 applications use
[xwayland-satellite](./Xwayland.md).

## Manual installation

See [Packaging swayward](./Packaging-swayward.md) for destination paths.
