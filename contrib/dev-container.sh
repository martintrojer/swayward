#!/usr/bin/env bash
# Create the swayward build container on an immutable Fedora host.
set -euo pipefail

IMAGE=registry.fedoraproject.org/fedora-toolbox:44
NAME=swayward-dev

if ! distrobox list | awk -F ' *\| *' -v name="$NAME" 'NR > 1 && $2 == name { found = 1 } END { exit !found }'; then
	distrobox create --name "$NAME" --image "$IMAGE"
fi

distrobox enter "$NAME" -- sudo dnf install -y \
	curl \
	gcc \
	clang \
	clang-devel \
	cargo \
	rustfmt \
	pkgconf-pkg-config \
	glib2-devel \
	pango-devel \
	cairo-devel \
	cairo-gobject-devel \
	libinput-devel \
	systemd-devel \
	libseat-devel \
	wayland-devel \
	libxkbcommon-devel \
	libdisplay-info-devel \
	pixman-devel \
	mesa-libgbm-devel \
	libdrm-devel \
	mesa-libEGL-devel \
	dbus-devel \
	pipewire-devel \
	libadwaita-devel \
	perl-JSON-PP \
	perl-Test-Simple

printf '%s\n' "Done. Build with:" \
	"  distrobox enter $NAME -- bash -lc 'cd $(pwd) && cargo build'"
