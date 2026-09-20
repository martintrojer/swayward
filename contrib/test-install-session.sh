#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")/.."

HOME=$(mktemp -d)
trap 'rm -rf "$HOME"' EXIT
export HOME

contrib/install-session.sh --debug >/dev/null

PORTAL="$HOME/.config/xdg-desktop-portal/swayward-portals.conf"
cmp resources/swayward-portals.conf "$PORTAL"
test -x "$HOME/.local/bin/start-swayward"

# Swayward deliberately defaults to the GNOME backend for its integrated
# window picker and dynamic cast targets. wlr remains an optional fallback,
# not the shipped policy.
grep -qx 'default=gnome;gtk;' "$PORTAL"
grep -qx 'org.freedesktop.impl.portal.ScreenCast=gnome;' "$PORTAL"
grep -qx 'org.freedesktop.impl.portal.Screenshot=gnome;' "$PORTAL"
if grep -q '=wlr' "$PORTAL"; then
	exit 1
fi

contrib/install-session.sh --uninstall >/dev/null
test ! -e "$PORTAL"

echo "install-session: portal selection installed and removed"
