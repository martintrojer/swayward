#!/usr/bin/env bash
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/home/.config/swayward" "$TMP/xdg/swayward" "$TMP/runtime"
printf 'USER_CONFIG_MARKER\n' > "$TMP/home/.config/swayward/config.kdl"
printf 'XDG_CONFIG_MARKER\n' > "$TMP/xdg/swayward/config.kdl"

cat > "$TMP/bin/distrobox" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
command=${!#}
printf '%s\n' "$command" | grep -q 'command -v systemd-run'
config=$(printf '%s\n' "$command" | sed -n "s/.* -c '\([^']*\)'.*/\1/p")
[ -n "$config" ]
[ -f "$config" ]
if [ -n "${EXPECT_CONFIG:-}" ]; then
    [ "$config" = "$EXPECT_CONFIG" ]
else
    cmp -s <(grep -v 'spawn-at-startup' resources/default-config.kdl) "$config"
    ! grep -q 'USER_CONFIG_MARKER\|XDG_CONFIG_MARKER\|spawn-at-startup' "$config"
fi
EOF
chmod +x "$TMP/bin/distrobox"

output=$(PATH="$TMP/bin:$PATH" HOME="$TMP/home" XDG_CONFIG_HOME="$TMP/xdg" \
    XDG_RUNTIME_DIR="$TMP/runtime" WAYLAND_DISPLAY=wayland-test \
    "$REPO/contrib/dev-run.sh" --keep-super --keep-client)
printf '%s\n' "$output" | grep -q "config:    $TMP/runtime/swayward-dev\."

# The cap and timeout must run inside the container. Wrapping distrobox itself
# only stops the podman client; the compositor keeps running in its cgroup.
! grep -q '^systemd-run .*distrobox enter' "$REPO/contrib/dev-run-desktop.sh"
grep -A8 '^distrobox enter .*bash -lc' "$REPO/contrib/dev-run-desktop.sh" |
    grep -q 'exec systemd-run'
grep -q 'systemctl --user show.*MemoryMax' "$REPO/contrib/dev-run.sh"
grep -q '/sys/fs/cgroup.*memory.max' "$REPO/contrib/dev-run-desktop.sh"

grep -q '/proc/net/unix' "$REPO/contrib/dev-run.sh"
grep -q 'kill .*REALPID' "$REPO/contrib/dev-run.sh"
custom=$TMP/custom.kdl
printf 'CUSTOM_CONFIG\n' >"$custom"
warning=$(PATH="$TMP/bin:$PATH" HOME="$TMP/home" XDG_CONFIG_HOME="$TMP/xdg" \
    XDG_RUNTIME_DIR="$TMP/runtime" WAYLAND_DISPLAY=wayland-test EXPECT_CONFIG="$custom" \
    "$REPO/contrib/dev-run.sh" --config "$custom" --keep-spawn --keep-super --keep-client 2>&1)
printf '%s\n' "$warning" | grep -q -- '--keep-spawn, --keep-super and --keep-client have no effect with --config'
