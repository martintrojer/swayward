# Build swayward

## Requirements

- Rust 1.87 or newer.
- The build dependencies your distribution ships for a wlroots-style
  compositor.

On Fedora:

```sh
sudo dnf install cargo gcc clang libudev-devel libgbm-devel libxkbcommon-devel \
    wayland-devel libinput-devel dbus-devel systemd-devel libseat-devel \
    libdisplay-info-devel pipewire-devel pango-devel cairo-gobject-devel \
    mesa-libEGL-devel libadwaita-devel perl-JSON-PP perl-Test-Simple
```

On Debian and Ubuntu:

```sh
sudo apt install curl gcc clang libudev-dev libgbm-dev libxkbcommon-dev \
    libegl1-mesa-dev libwayland-dev libinput-dev libdbus-1-dev libsystemd-dev \
    libseat-dev libdisplay-info-dev libpipewire-0.3-dev libpango1.0-dev
```

## Build and test

```sh
cargo build --release
cargo test --all
```

The i3 conformance suite runs as part of `cargo test` and needs Perl with
`JSON::PP` and `Test::More`. To skip it, test one crate at a time:

```sh
cargo test -p swayward --lib
```

## Build on an immutable host

Fedora Atomic and similar images cannot install build dependencies. Build in a
container instead:

```sh
./contrib/dev-container.sh
distrobox enter swayward-dev -- bash -lc 'cd "$PWD" && cargo build --release'
```

The script creates `swayward-dev` from `fedora-toolbox:44` and installs the
dependencies listed above. Distrobox mounts your home directory at the same
path, so run Cargo from the repository path inside the container.

## Run a nested session

To try swayward inside your current session, without replacing it:

```sh
contrib/dev-run.sh                 # 120 seconds, then reaped
contrib/dev-run.sh --timeout 600
contrib/dev-run.sh --config ~/my-config.kdl
```

The script copies the repository's default config into a private temporary
file unless you pass `--config`; it never discovers your user config. It caps
memory and wall time, unsets `SWAYSOCK` and `I3SOCK` so the nested compositor
cannot adopt the outer one's socket, and strips `spawn-at-startup` so a spawned
bar does not compete for the IPC socket. It rewrites `Super+` binds to `Mod+`,
because the outer compositor takes `Super` before the nested window sees it.

## Install as a login session

`contrib/install-session.sh` installs swayward as a selectable session without
copying anything into `/usr`:

```sh
cargo build --release
contrib/install-session.sh --config sway   # also converts ~/.config/sway/config
```

The binary and launcher go under `~/.local`; the config and portal selection go
under `~/.config`. One root-owned desktop file is unavoidable, because a
display manager only reads `/usr/share/wayland-sessions`. The script prints the
exact `sudo install` command and never runs it for you.

Before you log in to swayward for the first time:

- Keep a way out. `Ctrl+Alt+F3` reaches a TTY, and `pkill -9 swayward` returns
  you to your display manager.
- Check that your config parses. The script warns if it does not.
- Startup output goes to `~/.local/share/swayward/session.log`.
