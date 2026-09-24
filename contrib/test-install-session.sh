#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$(readlink -f "$0")")/.."

HOME=$(mktemp -d)
trap 'rm -rf "$HOME"' EXIT
export HOME
mkdir -p "$HOME/build"
cat >"$HOME/build/swayward" <<'EOF'
#!/bin/sh
[ "$1" = validate ] || [ "$1" = --version ]
[ "$1" != --version ] || echo 'swayward test'
EOF
cp "$HOME/build/swayward" "$HOME/build/swaywardmsg"
chmod +x "$HOME/build/swayward" "$HOME/build/swaywardmsg"

missing=$(PATH=/usr/bin:/bin contrib/install-session.sh --debug 2>&1 || true)
printf '%s\n' "$missing" | grep -q "build container 'swayward-dev' is missing"
! printf '%s\n' "$missing" | grep -q 'building\.\.\.'

SWAYWARD_BUILD_DIR="$HOME/build" contrib/install-session.sh --debug --no-build >/dev/null

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
