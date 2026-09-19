# swayward

**An i3/sway-compatible Wayland compositor, built in Rust on Smithay.**

> sway, gone its own way.

Swayward gives you i3's fully nested container tree on a modern Wayland
compositor: splits inside splits, tabbed and stacked containers, marks,
criteria, the scratchpad, and sway's IPC protocol on `SWAYSOCK` so your
existing tooling keeps working.

**Status: beta.** The compositor runs live sessions, speaks sway's IPC to real
clients, and passes 111 of i3's own test files unmodified. It is ready to try
as a daily driver if you know i3 or sway. Expect rough edges at the margins,
and read [known deviations](docs/KNOWN_DEVIATIONS.md) before you switch.

<!-- TODO(screenshots): replace these placeholders with real captures.
     Worth showing, in this order:
       1. a nested split — splitv inside splith, three or four terminals
       2. tabbed and stacked containers side by side, titlebars visible
       3. the hotkey overlay, with blur and rounded corners on
       4. the scratchpad mid-summon over a tiled workspace
     Capture on real hardware; every automated trial in this project is
     headless and cannot photograph pixels. -->

> **Screenshots pending.** Every automated trial in this project is headless,
> so the shots need capturing on real hardware before they go here. See the
> comment above for the four that are worth taking.

## Why the i3/sway model

This is a good era for Wayland compositors. Niri's scrollable strip is a
genuinely new idea and it is beautifully built — swayward is a fork of it, and
owes it nearly everything below the layout engine. Hyprland has pushed harder
on effects and configurability than anyone. Sway did the unglamorous work of
being the compositor people could actually rely on, and i3 defined the model
in the first place.

We still think i3's tree is the best window-management model there is, and
swayward exists because we wanted it on a modern foundation. Three reasons:

**The tree is what you already mean.** Every layout you want is a nesting of
"these share space horizontally" and "these share space vertically". That is
not an abstraction over window management, it *is* window management. Once you
see the tree, every keystroke follows from it, and the layout stops surprising
you.

**It composes without limit.** A container holds windows or other containers,
so tabs inside a split inside a tab costs nothing extra to learn — it is the
same rule applied again. Models built from special cases run out; a tree does
not.

**It is explicit, and it remembers.** Swayward does not guess where your next
window goes. You told it, possibly minutes ago, and it kept the note. That is
why an i3 layout is reproducible: the arrangement is a structure you built,
not an emergent property of the order you happened to open things in.

If you want to learn the model properly, the wiki has
[Sway School](docs/wiki/Sway-School.md) — a tree-first tutorial in 15 short
lessons with quizzes, using swayward's default keys. It is the fastest way from
"I don't get how layouts work" to seeing the tree.

And if the scrollable strip suits you better, use
[niri](https://github.com/YaLTeR/niri). It is excellent, it is where swayward
came from, and we are not going to pretend otherwise. Swayward deliberately
does not offer a scrollable mode.

## Install

### Fedora

```sh
sudo dnf copr enable martintrojer/swayward
sudo dnf install swayward
```

### Nix

```sh
nix profile install github:martintrojer/swayward
```

Or add the flake as an input and use the `swayward` package.

### Prebuilt tarball

Each [release](https://github.com/martintrojer/swayward/releases) ships an
`x86_64` tarball for distributions without a package yet:

```sh
tar xzf swayward-<version>-x86_64-linux-gnu.tar.gz
cd swayward-<version>-x86_64-linux-gnu
install -Dm755 bin/* -t ~/.local/bin/
```

The tarball is **not** self-contained: a compositor cannot statically link the
seat, input and GPU stack it drives. `RUNTIME.txt` inside the archive records
the exact glibc floor the binary was built against and what else to install.

### Arch

```sh
yay -S swayward       # or: paru -S swayward
```

The package source is [`contrib/PKGBUILD`](contrib/PKGBUILD); it builds and
lints clean in a stock Arch container.

### From source

See [Build swayward](docs/BUILDING.md). You need Rust 1.87 or newer and the
usual wlroots-style build dependencies.

## First run

Pick **Swayward** from your display manager, or run it nested inside an
existing session to try it without committing:

```sh
contrib/dev-run.sh          # capped, reaped on exit, 120s by default
contrib/dev-run.sh --timeout 600
```

The defaults give you a working session: `Mod` is Super, `Mod+Return` opens a
terminal, `Mod+D` runs a launcher, `Mod+Shift+Q` closes a window, and
`Mod+Shift+E` exits. The full set is in
[resources/default-config.kdl](resources/default-config.kdl), and the
[getting-started guide](docs/wiki/Getting-Started.md) walks through it.

Press `Mod+Shift+/` at any time for the hotkey overlay.

## Using it

Swayward exports `SWAYSOCK` and speaks sway's binary IPC, so the tools you
already have work:

```sh
swaywardmsg -t get_tree
swaywardmsg -t get_workspaces -p
swaywardmsg 'workspace 3'
swaywardmsg -t subscribe -m '["window"]'
```

`swaywardmsg` ships with swayward, so you do not need sway installed to drive
the socket. `swaymsg` works too if you have it.

**Waybar** works unmodified — point its `sway/*` modules at swayward and they
connect. Mako, swaybg, swayidle and swaylock all behave as they do under sway.
Scripts that speak sway IPC work to the extent they stay inside the implemented
surface; the [compatibility matrix](docs/SWAY_COMPATIBILITY.md) is the exact
boundary.

### Configuration

Swayward is **IPC-compatible, not config-compatible**. It is configured in
typed KDL, which is what lets it carry niri's animation and rendering settings
without inventing a second config language.

Bring an existing sway config across with the translator:

```sh
contrib/sway-to-kdl ~/.config/sway/config >config.kdl
swayward validate -c config.kdl
```

It reports what it cannot translate exactly instead of quietly dropping it.
See [Migrate a sway config](docs/SWAY_CONFIG_MIGRATION.md).

## How compatibility is measured

i3 ships 285 Perl test files. Swayward vendors 242 of them byte-for-byte from
a pinned i3 revision and runs them against a real headless compositor, real
Wayland clients and the sway IPC socket. The adapter replaces X11 window setup;
it does not edit upstream assertions.

At this revision **111 files pass in full**, unmodified. Across all 3,224
assertions: 2,349 pass and 875 are skips that each carry a reason and a
citation into sway's or i3's source. Nothing is left unexplained. The
[conformance report](tests/i3/README.md) has the assertion-level detail.

The report also records a green ceiling of 114 files: 111 currently green plus
3 vendored files blocked only by implementation or adapter gaps. Files needing
i3-only behaviour or unavailable input are excluded from it.

This is evidence, not a compatibility percentage. Roughly two thirds of the
skips are structural: X11-only assertions, i3's own parser binary, i3bar, and
tree nodes sway does not create either.

## What differs from sway

Read [known deviations](docs/KNOWN_DEVIATIONS.md) before migrating. The
headlines:

- **X11 identity is flattened.** Xwayland goes through
  `xwayland-satellite`, which presents ordinary `xdg_toplevel` surfaces, so
  separate X11 class, instance, role and XID do not cross the boundary.
- **No `bar {}` block.** Swayward does not launch or configure swaybar.
  Configure Waybar directly.
- **No in-place restart.** Reload is supported; replacing the process while
  keeping clients is not. Sway has no runtime `restart` either.
- **The IPC surface is incomplete.** Some requests, events, criteria and
  command forms are unimplemented and return structured failures rather than
  pretending to succeed.

## Project invariants

- Every `SWAYSOCK` reply is sway-shaped or a structured failure. It never hangs.
- The container tree stays well formed after every mutation.
- A live-session path must not panic, and an `ERROR` in the log is a bug.
- A feature needs executable evidence before it counts as working.
- Stability regressions outrank new features.

## Credits

Swayward is a fork of niri at commit `9e72e491`, and keeps its rendering,
backend, protocol and portal work. See [Fork base](docs/FORK-BASE.md).

- [niri](https://github.com/YaLTeR/niri) by Ivan Molodetskikh — the compositor
  this is built on. The debt is large and gladly acknowledged.
- [Smithay](https://github.com/Smithay/smithay) — the Wayland compositor
  toolkit underneath.
- [sway](https://github.com/swaywm/sway) — the IPC protocol, and the standard
  of reliability worth aiming at.
- [i3](https://i3wm.org) — the tree model, and the conformance suite that keeps
  us honest about it.

Licensed under **GPL-3.0-or-later**. The vendored i3 tests keep their upstream
BSD licence in [`tests/i3/LICENSE`](tests/i3/LICENSE).
