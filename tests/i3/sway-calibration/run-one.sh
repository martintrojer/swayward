#!/bin/bash
# Inner half of contrib/sway-calibration-run: runs ONE upstream i3 test file
# against sway, inside the container, inside the memory/time cap, inside a
# private user namespace. Not meant to be called directly.
#
# Required in the environment: I3SRC, STAGE, SWAY_CAL_SWAY, SWAY_CAL_RUNDIR,
# TESTNAME, OUTDIR.
set -uo pipefail

XVFB_PID=""
cleanup() {
	if [ -f "$SWAY_CAL_RUNDIR/pids" ]; then
		while read -r pid; do kill -9 -- "-$pid" 2>/dev/null; done < "$SWAY_CAL_RUNDIR/pids"
	fi
	[ -n "$XVFB_PID" ] && kill -9 "$XVFB_PID" 2>/dev/null
}
trap cleanup EXIT

# wlroots refuses an X socket dir it does not own
# (wlroots/xwayland/sockets.c:71-94), and in a distrobox /tmp/.X11-unix is
# owned by nobody. A private bind mount inside this namespace gives each run
# its own :0 and keeps it away from the host's X sockets entirely.
mkdir -p /tmp/.X11-unix
mount --bind "$SWAY_CAL_RUNDIR/x11u" /tmp/.X11-unix

export XDG_RUNTIME_DIR="$SWAY_CAL_RUNDIR"
unset SWAYSOCK I3SOCK WAYLAND_DISPLAY

# A bootstrap X server, purely to satisfy i3test::import.
#
# i3test.pm:139-153 opens its X connection and warps the pointer BEFORE it
# calls launch_with_config, because with i3 the X server already exists: i3 is
# a client of the Xvfb that StartXServer.pm started. Sway inverts that --- its
# Xwayland does not exist until sway does --- so import has no server to talk
# to and dies at i3test.pm:157.
#
# Xvfb on :99 gives import a server to connect to and warp a pointer on.
# Nothing under test ever uses it: SocketActivation rebinds $i3test::x to
# sway's own Xwayland as soon as sway is up, before the first assertion.
# Its geometry matches StartXServer.pm:106-108 so that nothing observes a
# different screen size in the window between import and that rebind.
XVFB_DISPLAY=:99
Xvfb "$XVFB_DISPLAY" -screen 0 1280x800x24 >/dev/null 2>&1 &
XVFB_PID=$!
export DISPLAY=$XVFB_DISPLAY
for _ in $(seq 1 100); do [ -S "/tmp/.X11-unix/X99" ] && break; sleep 0.05; done

cd "$I3SRC/testcases" || exit 1
perl -I"$STAGE" -Mswaycal::Shim "t/$TESTNAME"
exit $?
