#!/usr/bin/env bash
set -euo pipefail

if (( $# < 1 || $# > 2 )); then
    echo "usage: $0 /path/to/nested-sway-ipc.sock [multi-floating]" >&2
    exit 2
fi

TARGET_SWAYSOCK=$1
SCENARIO=${2-all}
AMBIENT_SWAYSOCK=${SWAYSOCK-}
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
OUT=$ROOT/tests/fixtures/sway

if [[ -z $TARGET_SWAYSOCK || ! -S $TARGET_SWAYSOCK ]]; then
    echo "target is not a sway IPC socket: $TARGET_SWAYSOCK" >&2
    exit 2
fi
if [[ -n $AMBIENT_SWAYSOCK && $TARGET_SWAYSOCK == "$AMBIENT_SWAYSOCK" ]]; then
    echo "refusing to modify the ambient sway session: $TARGET_SWAYSOCK" >&2
    exit 2
fi

mkdir -p "$OUT"

msg() {
    swaymsg -s "$TARGET_SWAYSOCK" -r "$@"
}

command_ok() {
    jq -e 'type == "array" and all(.success == true)' >/dev/null
}

run_command() {
    local reply
    reply=$(msg "$@")
    if ! command_ok <<<"$reply"; then
        jq . <<<"$reply" >&2
        return 1
    fi
}

kill_fixture_windows() {
    local reply
    reply=$(msg '[app_id="^fixture-"] kill') || true
    jq -e 'type == "array" and all(.success == true or .error == "No matching node.")' >/dev/null <<<"$reply"
}

window_count() {
    msg -t get_tree | jq '[recurse(.nodes[], .floating_nodes[]; true) | select((.app_id? // "") | startswith("fixture-"))] | length'
}

wait_for_windows() {
    local wanted=$1
    for _ in {1..100}; do
        [[ $(window_count) -eq $wanted ]] && return 0
        sleep 0.05
    done
    echo "timed out waiting for $wanted fixture windows" >&2
    return 1
}

spawn_window() {
    local id=$1
    run_command exec "env WAYLAND_DISPLAY=$WAYLAND_DISPLAY foot --app-id=$id --title=$id sh -c 'sleep 300'"
}

reset_state() {
    kill_fixture_windows
    run_command workspace __fixture_reset
    run_command workspace 1
    wait_for_windows 0
}

capture() {
    local name=$1
    sleep 0.15
    msg -t get_tree | jq -S . >"$OUT/$name.tree.json"
    msg -t get_workspaces | jq -S . >"$OUT/$name.workspaces.json"
    msg -t get_outputs | jq -S . >"$OUT/$name.outputs.json"
    printf 'captured %s\n' "$name"
}

empty() {
    reset_state
    capture empty
}

one_window() {
    reset_state
    spawn_window fixture-1
    wait_for_windows 1
    capture one_window
}

two_split_h() {
    reset_state
    run_command splith
    spawn_window fixture-1
    spawn_window fixture-2
    wait_for_windows 2
    capture two_split_h
}

two_split_v() {
    reset_state
    run_command splitv
    spawn_window fixture-1
    spawn_window fixture-2
    wait_for_windows 2
    capture two_split_v
}

nested_h_in_v() {
    reset_state
    spawn_window fixture-1
    wait_for_windows 1
    run_command splitv
    spawn_window fixture-2
    wait_for_windows 2
    run_command splith
    spawn_window fixture-3
    wait_for_windows 3
    capture nested_h_in_v
}

tabbed() {
    reset_state
    run_command 'layout tabbed'
    spawn_window fixture-1
    spawn_window fixture-2
    wait_for_windows 2
    capture tabbed
}

stacked() {
    reset_state
    run_command 'layout stacking'
    spawn_window fixture-1
    spawn_window fixture-2
    wait_for_windows 2
    capture stacked
}

one_floating() {
    reset_state
    spawn_window fixture-1
    spawn_window fixture-2
    wait_for_windows 2
    run_command floating enable
    capture one_floating
}

two_floating() {
    reset_state
    spawn_window fixture-1
    wait_for_windows 1
    run_command floating enable
    spawn_window fixture-2
    wait_for_windows 2
    run_command floating enable
    capture two_floating
}

three_floating_raise() {
    reset_state
    spawn_window fixture-tiled
    wait_for_windows 1
    local count=1
    for id in fixture-1 fixture-2 fixture-3; do
        spawn_window "$id"
        count=$((count + 1))
        wait_for_windows "$count"
        run_command floating enable
    done
    capture three_floating_before_raise
    run_command '[app_id="^fixture-1$"] focus'
    capture three_floating_after_raise
}

fullscreen() {
    reset_state
    spawn_window fixture-1
    wait_for_windows 1
    run_command fullscreen enable
    capture fullscreen
}

marked() {
    reset_state
    spawn_window fixture-1
    wait_for_windows 1
    run_command 'mark testmark'
    capture marked
}

two_workspaces() {
    reset_state
    spawn_window fixture-1
    wait_for_windows 1
    run_command workspace 2
    spawn_window fixture-2
    wait_for_windows 2
    capture two_workspaces
}

numbered_sparse() {
    reset_state
    spawn_window fixture-1
    wait_for_windows 1
    run_command workspace 3
    spawn_window fixture-3
    wait_for_windows 2
    run_command workspace 7
    spawn_window fixture-7
    wait_for_windows 3
    capture numbered_sparse
}

named_workspace() {
    reset_state
    run_command workspace mail
    spawn_window fixture-mail
    wait_for_windows 1
    capture named_workspace
}

urgent() {
    reset_state
    # foot marks a terminal urgent when an unfocused client emits BEL.
    run_command exec "foot --app-id=fixture-urgent --title=fixture-urgent sh -c 'sleep 1; printf \\a; sleep 300'"
    wait_for_windows 1
    spawn_window fixture-focus
    wait_for_windows 2
    for _ in {1..60}; do
        if msg -t get_tree | jq -e '.. | objects | select(.app_id? == "fixture-urgent") | .urgent == true' >/dev/null; then
            capture urgent
            return
        fi
        sleep 0.1
    done
    echo "urgent: foot did not expose an urgency hint; no fixture captured" >&2
}

empty_named() {
    reset_state
    run_command workspace mail
    capture empty_named
}

main() {
    local version
    version=$(msg -t get_version | jq -r '.human_readable')
    echo "capturing from sway $version at $TARGET_SWAYSOCK"
    if [[ $SCENARIO == multi-floating ]]; then
        two_floating
        three_floating_raise
        reset_state
        return
    fi
    if [[ $SCENARIO != all ]]; then
        echo "unknown scenario set: $SCENARIO" >&2
        exit 2
    fi
    rm -f "$OUT"/*.json
    empty
    one_window
    two_split_h
    two_split_v
    nested_h_in_v
    tabbed
    stacked
    one_floating
    two_floating
    three_floating_raise
    fullscreen
    marked
    two_workspaces
    numbered_sparse
    named_workspace
    urgent
    empty_named
    reset_state
}

main
