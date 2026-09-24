#!/usr/bin/env bash
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
SESSION=$REPO/resources/swayward-session
EXPECTED='WAYLAND_DISPLAY DISPLAY XDG_SESSION_TYPE XDG_CURRENT_DESKTOP SWAYSOCK I3SOCK'

grep -Fqx "    systemctl --user unset-environment $EXPECTED" "$SESSION"
grep -Fqx "    dinitctl --quiet --user unsetenv $EXPECTED 2>/dev/null" "$SESSION"
! grep -q 'SWAYWARD_SOCKET' "$SESSION"
