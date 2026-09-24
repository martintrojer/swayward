#!/usr/bin/env bash
# Launch a nested swayward that looks like your desktop: your wallpaper, your
# Waybar, and a window sized for clean screenshots.
#
#   contrib/dev-run-desktop.sh                  # 1280x800 logical, Wayland scale 1
#   contrib/dev-run-desktop.sh --size 1600x1000 # a different logical size
#   contrib/dev-run-desktop.sh --scale 2        # Wayland scale 2: same layout,
#                                               # screenshots at 2560x1600
#   contrib/dev-run-desktop.sh --no-bar         # skip Waybar
#   contrib/dev-run-desktop.sh --timeout 1800   # longer session
#   contrib/dev-run-desktop.sh --config path    # your own config
#
# --size is the nested output's LOGICAL size and --scale its Wayland output
# scale, so a screenshot is exactly size x scale pixels whatever your host's own
# scale is. The host window is sized to size x scale / host-scale to get there.
#
# Screenshots: the nested output is named "winit". Capture it with
#   WAYLAND_DISPLAY=<nested> grim -o winit shot.png
# The script prints the exact command once the session is up.
#
# This keeps every safety rule from contrib/dev-run.sh (memory cap with no
# swap, wall-clock timeout, never adopting the outer SWAYSOCK, reaping only
# what it started). Read AGENTS.md before relaxing any of them.
set -uo pipefail

cd "$(dirname "$(readlink -f "$0")")/.." || exit
REPO=$PWD

TIMEOUT=600
MEMMAX=2G
SIZE=1280x800
SCALE=1
BAR=1
CONFIG=""
CONTAINER=swayward-dev
PROFILE=debug

while [ $# -gt 0 ]; do
	case "$1" in
	--timeout) TIMEOUT=$2; shift 2 ;;
	--memmax) MEMMAX=$2; shift 2 ;;
	--size) SIZE=$2; shift 2 ;;
	--scale) SCALE=$2; shift 2 ;;
	--no-bar) BAR=0; shift ;;
	--config) CONFIG=$2; shift 2 ;;
	--release) PROFILE=release; shift ;;
	-h | --help) sed -n '2,18p' "$0"; exit 0 ;;
	*) echo "unknown option: $1" >&2; exit 2 ;;
	esac
done

case "$SIZE" in
*x*) WIDTH=${SIZE%x*}; HEIGHT=${SIZE#*x} ;;
*) echo "--size must look like 1280x800" >&2; exit 2 ;;
esac

if [ -z "${WAYLAND_DISPLAY:-}" ]; then
	echo "WAYLAND_DISPLAY is unset; run this from inside your Wayland session." >&2
	exit 1
fi
HOST_SWAYSOCK=${SWAYSOCK:-}

# A nested compositor must never adopt the outer session's IPC socket
# (src/ipc/server.rs select_socket_path; see AGENTS.md).
unset SWAYSOCK I3SOCK

RUNDIR=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/swayward-desktop.XXXXXX")
PIDS=""
REALPID=""
cleanup() {
	for pid in $PIDS; do kill "$pid" 2>/dev/null; done
	# The compositor runs inside the container via 'distrobox enter', and
	# killing that client does not stop the process it started. Reap the real
	# swayward pid, taken from its socket name, but only after checking its
	# command line is our binary with our config: a pid can be reused.
	if [ -n "$REALPID" ] &&
		tr '\0' ' ' </proc/"$REALPID"/cmdline 2>/dev/null | grep -qF -- "-c $CONFIG"; then
		kill "$REALPID" 2>/dev/null
		for _ in 1 2 3 4 5 6 7 8; do kill -0 "$REALPID" 2>/dev/null || break; sleep 0.25; done
		kill -9 "$REALPID" 2>/dev/null
	fi
	# Remove only the socket we observed, and only once nothing listens on it.
	if [ -n "${SOCK:-}" ] && [ -S "$SOCK" ] && ! grep -qF "$(basename "$SOCK")" /proc/net/unix; then
		rm -f "$SOCK"
	fi
	rm -rf "$RUNDIR"
}
trap cleanup EXIT

# --- wallpaper: the same image your session-wallpaper service uses ---------
WALLPAPER=""
if [ -x "$HOME/.local/bin/wallpaper" ]; then
	WALLPAPER=$("$HOME/.local/bin/wallpaper" current 2>/dev/null) || WALLPAPER=""
fi
if [ -z "$WALLPAPER" ]; then
	# Fall back to whatever swaybg the host is running right now.
	WALLPAPER=$(pgrep -a swaybg | grep -oE -- '-i [^ ]+' | head -1 | cut -d' ' -f2)
fi
[ -n "$WALLPAPER" ] && [ ! -f "$WALLPAPER" ] && WALLPAPER=""

# --- config ------------------------------------------------------------------
if [ -z "$CONFIG" ]; then
	SRC=resources/default-config.kdl
	[ -f "$HOME/.config/swayward/config.kdl" ] && SRC="$HOME/.config/swayward/config.kdl"
	CONFIG="$RUNDIR/config.kdl"
	# Bars and wallpaper are started by this script, not by the config, so a
	# spawned copy cannot race ours for the IPC socket.
	grep -v 'spawn-at-startup' "$SRC" >"$CONFIG"
	# The outer compositor grabs Super; nested binds must use Mod (Alt here).
	sed -i 's/\bSuper+/Mod+/g; s/footclient/foot/g' "$CONFIG"
	# The first-run help card would sit in the middle of every screenshot.
	sed -i 's|^\(\s*\)// skip-at-startup|\1skip-at-startup|' "$CONFIG"
	# Scale 1 by default: a nested window on a fractionally scaled host is
	# otherwise rendered at the host's scale, which blurs a 1:1 screenshot.
	printf '\noutput "winit" {\n    scale %s\n}\n' "$SCALE" >>"$CONFIG"
fi

BIN=./target/$PROFILE/swayward
BUILD="cargo build -p swayward --bin swayward -p swayward-ipc --bin swaywardmsg"
[ "$PROFILE" = release ] && BUILD="$BUILD --release"
echo "config:    $CONFIG"
echo "wallpaper: ${WALLPAPER:-none found, using background colour}"
echo "output:    ${WIDTH}x${HEIGHT} logical at Wayland scale $SCALE"
echo "caps:      MemoryMax=$MEMMAX MemorySwapMax=0 timeout ${TIMEOUT}s"
echo

# Build in the container; everything below runs from one shell so the reaper
# can see every process it started.
distrobox enter "$CONTAINER" -- bash -lc "cd '$REPO' && $BUILD" || exit 1

STAMP=$(mktemp)
distrobox enter "$CONTAINER" -- bash -lc "
command -v systemd-run >/dev/null || {
  echo 'systemd-run is missing in the build container; rerun contrib/dev-container.sh' >&2
  exit 1
}
unset SWAYSOCK I3SOCK
exec systemd-run --user --scope --quiet \\
  -p MemoryMax='$MEMMAX' -p MemorySwapMax=0 \\
  timeout '$TIMEOUT' '$REPO/$BIN' -c '$CONFIG'
" >"$RUNDIR/swayward.log" 2>&1 &
NESTED_PID=$!
PIDS="$PIDS $NESTED_PID"

SOCK=""
for _ in $(seq 1 120); do
	SOCK=$(find "/run/user/$UID" -maxdepth 1 -name 'swayward-ipc.*.sock' \
		-newer "$STAMP" -printf '%T@ %p\n' 2>/dev/null |
		sort -rn | head -1 | cut -d' ' -f2-)
	[ -n "$SOCK" ] && break
	kill -0 "$NESTED_PID" 2>/dev/null || break
	sleep 0.25
done
rm -f "$STAMP"
if [ -z "$SOCK" ]; then
	echo "no IPC socket appeared; see the log:" >&2
	tail -20 "$RUNDIR/swayward.log" >&2
	exit 1
fi
NESTED_DISPLAY=$(basename "$SOCK" | cut -d. -f2)
REALPID=$(basename "$SOCK" | cut -d. -f3)
CGROUP=$(awk -F: '$1 == 0 { print $3 }' "/proc/$REALPID/cgroup")
if [ -z "$CGROUP" ] || [ "$(cat "/sys/fs/cgroup$CGROUP/memory.max" 2>/dev/null)" = max ] ||
	[ "$(cat "/sys/fs/cgroup$CGROUP/memory.swap.max" 2>/dev/null)" != 0 ]; then
	echo "swayward started without the requested memory cap; see the log:" >&2
	tail -20 "$RUNDIR/swayward.log" >&2
	exit 1
fi

# Resize the nested window to the requested size, by its exact pid, so a
# broad criterion never touches any other window in your session.
if [ -n "$HOST_SWAYSOCK" ]; then
	# The host renders our window at its own output scale. Choose the window's
	# host-logical size so that host pixels equal size x scale, which is the
	# nested output's physical size; at Wayland scale SCALE that is SIZE
	# logical.
	HOST_SCALE=$(swaymsg -s "$HOST_SWAYSOCK" -t get_outputs -r 2>/dev/null |
		jq -r '[.[] | select(.focused)][0].scale // 1')
	HOST_W=$(awk -v w="$WIDTH" -v s="$SCALE" -v h="$HOST_SCALE" 'BEGIN{printf "%d", w*s/h + 0.5}')
	HOST_H=$(awk -v w="$HEIGHT" -v s="$SCALE" -v h="$HOST_SCALE" 'BEGIN{printf "%d", w*s/h + 0.5}')
	for _ in $(seq 1 40); do
		CON_ID=$(swaymsg -s "$HOST_SWAYSOCK" -t get_tree -r 2>/dev/null |
			jq -r --argjson pid "$REALPID" '.. | objects | select(.pid? == $pid) | .id' |
			head -1)
		[ -n "$CON_ID" ] && break
		sleep 0.25
	done
	if [ -n "${CON_ID:-}" ]; then
		swaymsg -s "$HOST_SWAYSOCK" "[con_id=$CON_ID] floating enable" >/dev/null
		swaymsg -s "$HOST_SWAYSOCK" \
			"[con_id=$CON_ID] resize set width $HOST_W px height $HOST_H px" >/dev/null
		swaymsg -s "$HOST_SWAYSOCK" "[con_id=$CON_ID] move position center" >/dev/null
	fi
fi

# --- wallpaper and bar, as clients of the nested session ----------------------
if [ -n "$WALLPAPER" ]; then
	WAYLAND_DISPLAY=$NESTED_DISPLAY swaybg -o winit -i "$WALLPAPER" -m fill \
		>"$RUNDIR/swaybg.log" 2>&1 &
else
	WAYLAND_DISPLAY=$NESTED_DISPLAY swaybg -o winit -c '#1e1e2e' \
		>"$RUNDIR/swaybg.log" 2>&1 &
fi
PIDS="$PIDS $!"

if [ "$BAR" = 1 ]; then
	if command -v waybar >/dev/null; then
		# Waybar finds the compositor through SWAYSOCK; point it at the nested
		# socket explicitly so it can never attach to your real session.
		WAYLAND_DISPLAY=$NESTED_DISPLAY SWAYSOCK=$SOCK waybar \
			>"$RUNDIR/waybar.log" 2>&1 &
		PIDS="$PIDS $!"
	else
		echo "waybar not found on the host; continuing without a bar" >&2
	fi
fi

sleep 0.5
ACTUAL=$(SWAYSOCK=$SOCK swaymsg -t get_outputs -r 2>/dev/null |
	jq -r '.[] | select(.name=="winit") | "\(.rect.width)x\(.rect.height) logical, scale \(.scale), \(.current_mode.width)x\(.current_mode.height) px"')
cat <<EOF
nested session is up
  output:   ${ACTUAL:-unknown}  (asked for ${WIDTH}x${HEIGHT} at scale $SCALE)
  display:  $NESTED_DISPLAY
  socket:   $SOCK

screenshot the whole nested output:
  WAYLAND_DISPLAY=$NESTED_DISPLAY grim -o winit shot.png

open a terminal in it:
  WAYLAND_DISPLAY=$NESTED_DISPLAY foot

query it (the nested socket, never your session's):
  SWAYSOCK=$SOCK swaymsg -t get_tree

close the window or press Ctrl+C here to end the session.
EOF

wait "$NESTED_PID"
STATUS=$?
[ "$STATUS" = 124 ] && echo "session ended: hit the ${TIMEOUT}s cap (use --timeout N)"
exit "$STATUS"
