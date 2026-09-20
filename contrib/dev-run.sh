#!/usr/bin/env bash
# Launch swayward nested inside your current Wayland session, capped so a
# runaway cannot take the machine down, and reaped on exit.
#
#   contrib/dev-run.sh                 # 120s, default config minus spawn-at-startup
#   contrib/dev-run.sh --timeout 600   # longer session
#   contrib/dev-run.sh --config path   # your own config
#   contrib/dev-run.sh --keep-spawn    # keep spawn-at-startup (competes for IPC)
#   contrib/dev-run.sh --keep-super    # keep literal Super+ binds (host grabs them)
#   contrib/dev-run.sh --keep-client   # keep footclient binds (they spawn on the host)
#   contrib/dev-run.sh -- --debug-flag # everything after -- goes to swayward
#
# Every rule here comes from an incident recorded in AGENTS.md. Read that file
# before changing any of it.
set -uo pipefail

cd "$(dirname "$(readlink -f "$0")")/.." || exit
REPO=$PWD

TIMEOUT=120
MEMMAX=2G
CONFIG=""
KEEP_SPAWN=0
KEEP_SUPER=0
KEEP_CLIENT=0
CONTAINER=swayward-dev
PROFILE=debug

while [ $# -gt 0 ]; do
	case "$1" in
	--timeout) TIMEOUT=$2; shift 2 ;;
	--memmax) MEMMAX=$2; shift 2 ;;
	--config) CONFIG=$2; shift 2 ;;
	--keep-spawn) KEEP_SPAWN=1; shift ;;
	--keep-super) KEEP_SUPER=1; shift ;;
	--keep-client) KEEP_CLIENT=1; shift ;;
	--release) PROFILE=release; shift ;;
	--) shift; break ;;
	-h | --help) sed -n '2,14p' "$0"; exit 0 ;;
	*) echo "unknown option: $1" >&2; exit 2 ;;
	esac
done

if [ -z "${WAYLAND_DISPLAY:-}" ]; then
	echo "WAYLAND_DISPLAY is unset: swayward picks its backend from it, and" >&2
	echo "without it this would try to take over a TTY. Run from inside your" >&2
	echo "Wayland session." >&2
	exit 1
fi

# A nested compositor must never adopt the outer session's IPC socket.
# select_socket_path (src/ipc/server.rs:99) adopts $SWAYSOCK when no file
# exists at that path, and a stale path plus a live listener is how a test run
# once hijacked the operator's real sway session and blanked their display.
unset SWAYSOCK I3SOCK

RUNDIR=$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/swayward-dev.XXXXXX")
trap 'rm -rf "$RUNDIR"' EXIT

if [ -z "$CONFIG" ]; then
	SRC=resources/default-config.kdl
	CONFIG="$RUNDIR/config.kdl"
	if [ "$KEEP_SPAWN" = 1 ]; then
		cp "$SRC" "$CONFIG"
	else
		# A spawned bar competes for the IPC socket and can starve swaymsg.
		grep -v 'spawn-at-startup' "$SRC" > "$CONFIG"
	fi

	# The outer compositor grabs Super before the nested window ever sees it,
	# so a config full of literal 'Super+h' binds looks completely dead in
	# here. Only 'Mod+' binds get remapped to the nested mod key (Alt, see
	# Backend::mod_key in src/backend/mod.rs), so rewrite Super+ to Mod+.
	# Verified by injecting Alt+Return with wtype: without this the keystroke
	# matches no bind at all.
	if [ "$KEEP_SUPER" = 0 ] && grep -q 'Super+' "$CONFIG"; then
		n=$(grep -c 'Super+' "$CONFIG")
		sed -i 's/\bSuper+/Mod+/g' "$CONFIG"
		echo "rewrote $n 'Super+' binds to 'Mod+' (nested mod key is Alt)"
	fi

	# footclient hands the request to the operator's existing 'foot --server',
	# which opens the window in THEIR session. The bind fires, the nested
	# window stays empty, and it reads as a broken keybind.
	if [ "$KEEP_CLIENT" = 0 ] && grep -q 'footclient' "$CONFIG"; then
		n=$(grep -c 'footclient' "$CONFIG")
		# Plain 'foot' is already server-less. Do NOT use
		# --server-socket=/dev/null here: that is a foot --server/footclient
		# flag and plain foot exits with "unrecognized option".
		sed -i 's/footclient/foot/g' "$CONFIG"
		echo "rewrote $n 'footclient' spawns to plain 'foot'"
	fi
fi

BIN=./target/$PROFILE/swayward
BUILD="cargo build"
[ "$PROFILE" = release ] && BUILD="cargo build --release"

echo "config:    $CONFIG"
echo "binary:    $BIN"
echo "caps:      MemoryMax=$MEMMAX MemorySwapMax=0 timeout ${TIMEOUT}s"
echo

# One script, one lifetime: build, launch, report and reap in a single shell,
# because a pkill in a later shell cannot see what an earlier one started.
# That is how a dozen orphaned compositors once accumulated unnoticed.
distrobox enter "$CONTAINER" -- bash -lc "
set -uo pipefail
cd '$REPO'
$BUILD || exit 1

# Reap only our own pid. 'pkill -f swayward' also matches this command line
# and the operator's live session.
cleanup() {
  [ -n \"\${NESTED_PID:-}\" ] && kill -9 \"\$NESTED_PID\" 2>/dev/null
  # Delete only the socket we actually observed this run. NESTED_PID is the
  # systemd-run/timeout pid, never the swayward pid embedded in the socket
  # name, so keying the glob off it matched nothing and leaked every run.
  # A blanket glob is not an option: it deletes live sockets belonging to a
  # concurrent test suite, and unlinking does not stop the server behind it.
  [ -n \"\${SOCK:-}\" ] && rm -f \"\$SOCK\" 2>/dev/null
}
trap cleanup EXIT

# Marker for 'which socket is ours': anything older than this predates us.
STAMP=\$(mktemp)

unset SWAYSOCK I3SOCK
systemd-run --user --scope --quiet \
  -p MemoryMax=$MEMMAX -p MemorySwapMax=0 \
  timeout $TIMEOUT '$BIN' -c '$CONFIG' ${*:+$*} &
NESTED_PID=\$!

# Wait for the IPC socket so we can tell you how to talk to it. 'ls -t |
# head -1' would happily hand back a dead socket from an earlier run, which
# then looks exactly like a compositor bug; require ours to postdate STAMP.
for _ in \$(seq 1 60); do
  SOCK=\$(find \"/run/user/\$UID\" -maxdepth 1 -name 'swayward-ipc.*.sock' \\
            -newer \"\$STAMP\" -printf '%T@ %p\\n' 2>/dev/null |
          sort -rn | head -1 | cut -d' ' -f2-)
  [ -n \"\$SOCK\" ] && break
  sleep 0.25
done
rm -f \"\$STAMP\"

if [ -n \"\${SOCK:-}\" ]; then
  # swayward-ipc.<display>.<pid>.<id>.sock -- take the display from the name
  # rather than guessing 'wayland-1', which is usually the OUTER session.
  DISP=\$(basename \"\$SOCK\" | cut -d. -f2)
  echo
  echo 'nested session is up. From another terminal, inside the container:'
  echo \"  export SWAYSOCK=\$SOCK\"
  echo '  swaymsg -t get_tree'
  echo
  echo 'launch a client into it (plain foot; footclient would open in YOUR session):'
  echo \"  WAYLAND_DISPLAY=\$DISP foot\"
  echo
else
  echo 'no IPC socket appeared; the compositor may have failed to start' >&2
fi

wait \$NESTED_PID
"
STATUS=$?

# timeout(1) reports 124 when it stops a session that was still running.
[ $STATUS = 124 ] && echo "session ended: hit the ${TIMEOUT}s cap (use --timeout N for longer)"
exit $STATUS
