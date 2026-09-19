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

# The policy content is load-bearing, not merely its presence. Without an
# explicit wlroots backend for these two interfaces, screen sharing and the
# screenshot portal find no backend and fail silently, which a user reports as
# "screen sharing just does nothing".
grep -qx 'org.freedesktop.impl.portal.ScreenCast=wlr' "$PORTAL"
grep -qx 'org.freedesktop.impl.portal.Screenshot=wlr' "$PORTAL"
# Preferring a backend that is absent on a typical wlroots system would push
# every interface onto a fallback. sway's own shipped policy defaults to gtk,
# and swayward claims sway compatibility, so it must not diverge here.
grep -qx 'default=gtk' "$PORTAL"
! grep -q '^default=gnome' "$PORTAL"

contrib/install-session.sh --uninstall >/dev/null
test ! -e "$PORTAL"

echo "install-session: portal selection installed and removed"
