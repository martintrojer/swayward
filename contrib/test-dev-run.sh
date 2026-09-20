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
config=$(printf '%s\n' "$command" | sed -n "s/.* -c '\([^']*\)'.*/\1/p")
[ -n "$config" ]
[ -f "$config" ]
cmp -s <(grep -v 'spawn-at-startup' resources/default-config.kdl) "$config"
! grep -q 'USER_CONFIG_MARKER\|XDG_CONFIG_MARKER\|spawn-at-startup' "$config"
EOF
chmod +x "$TMP/bin/distrobox"

output=$(PATH="$TMP/bin:$PATH" HOME="$TMP/home" XDG_CONFIG_HOME="$TMP/xdg" \
    XDG_RUNTIME_DIR="$TMP/runtime" WAYLAND_DISPLAY=wayland-test \
    "$REPO/contrib/dev-run.sh" --keep-super --keep-client)
printf '%s\n' "$output" | grep -q "config:    $TMP/runtime/swayward-dev\."
