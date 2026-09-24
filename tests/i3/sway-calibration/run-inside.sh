#!/bin/bash
# Container-side half of contrib/sway-calibration-run. Stages upstream
# i3test.pm, overlays the two sway-side files, then runs each requested test
# under the memory and time caps. Not meant to be called directly.

set -uo pipefail
# Never let anything adopt the ambient socket (AGENTS.md).
unset SWAYSOCK I3SOCK DISPLAY WAYLAND_DISPLAY

RUNROOT=$(mktemp -d /tmp/swaycal-run.XXXXXX)
export RUNROOT
cleanup() {
  # run-one.sh reaps only pids recorded beneath its own private run directory.
  # Do not pkill by binary here: that can kill a concurrent calibration run.
  rm -rf "$RUNROOT"
}
trap cleanup EXIT

# Build i3test.pm from upstream i3test.pm.in. The only substitutions are the
# two @...@ build placeholders meson fills in; nothing else is touched, and
# the result is diffed against i3 HEAD below.
export STAGE=$RUNROOT/lib
mkdir -p "$STAGE"
cp -r "$I3SRC/testcases/lib/." "$STAGE/"
sed -e "s,@abs_top_builddir@,$I3SRC,g" -e "s,@abs_top_srcdir@,$I3SRC,g" \
    "$I3SRC/testcases/lib/i3test.pm.in" > "$STAGE/i3test.pm"
sed -i -e "s,@abs_top_builddir@,$I3SRC,g" "$STAGE/i3test/XTEST.pm"
rm -f "$STAGE/i3test.pm.in"

# Overlay ONLY the two sway-side files. Print what is being replaced so the
# report can state its own method.
cp "$REPO/tests/i3/sway-calibration/SocketActivation.pm" "$STAGE/SocketActivation.pm"
mkdir -p "$STAGE/swaycal"
cp "$REPO/tests/i3/sway-calibration/swaycal/Shim.pm" "$STAGE/swaycal/Shim.pm"

export PERL5LIB="$STAGE:$HOME/perl5/lib/perl5${PERL5LIB:+:$PERL5LIB}"
export SWAY_CAL_SWAY="$SWAYBUILD/sway/sway"
export SWAY_CAL_CONTENT_SHIM="${SWAY_CAL_CONTENT_SHIM:-}"

while read -r t; do
  [ -f "$I3SRC/testcases/t/$t" ] || { echo "MISSING-UPSTREAM $t"; continue; }
  RUNDIR=$RUNROOT/$t
  mkdir -p "$RUNDIR/x11u" && chmod 1777 "$RUNDIR/x11u"
  export SWAY_CAL_RUNDIR="$RUNDIR" OUTDIR="$RUNDIR" TESTNAME="$t"
  chmod 700 "$RUNDIR"
  # XDG_RUNTIME_DIR is NOT overridden out here: systemd-run --user needs the
  # session one to reach the user bus. It is switched to the private dir
  # inside the namespace, so sway's wayland socket lands there and no socket
  # of the operator's is ever touched.

  # Each file gets its own user namespace so its Xwayland owns a private
  # /tmp/.X11-unix and can always take :0. The body lives in run-one.sh:
  # three levels of shell quoting in one heredoc silently mangled both the
  # memory cap and the loop, so the inner half is a real file.
  systemd-run --user --scope -q -p MemoryMax=$MEMMAX -p MemorySwapMax=0 \
    timeout --kill-after=5 "$TIMEOUT" \
    unshare -rm "$REPO/tests/i3/sway-calibration/run-one.sh" \
    > "$OUT/$t.tap" 2>&1
  echo "EXIT=$? $t"
  # The sway log is the only way to attribute a timeout, so keep it.
  cat "$RUNDIR"/sway-log-* > "$OUT/$t.swaylog" 2>/dev/null
  rm -rf "$RUNDIR"
done < "$OUT/.files"
