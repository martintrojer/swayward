#!/usr/bin/env bash
set -euo pipefail

if (( $# < 1 || $# > 2 )); then
    echo "usage: $0 /path/to/nested-sway-ipc.sock [inputs|inputs-libinput|multi-floating|event-sequences|cross-output-events|window-map-events]" >&2
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

capture_event_sequence() {
    local name=$1
    shift
    local stream
    stream=$(mktemp)
    swaymsg -s "$TARGET_SWAYSOCK" -t subscribe -m '["workspace","window","tick"]' >"$stream" &
    local subscriber=$!
    trap 'kill "$subscriber" 2>/dev/null || true; rm -f "$stream"' RETURN
    for _ in {1..100}; do
        kill -0 "$subscriber" 2>/dev/null || return 1
        if [[ -s $stream ]] && jq -e 'select(.first? == true)' "$stream" >/dev/null 2>&1; then
            break
        fi
        sleep 0.01
    done
    "$@"
    msg -t send_tick "fixture-$name" >/dev/null
    for _ in {1..100}; do
        jq -e --arg payload "fixture-$name" 'select(.payload? == $payload)' "$stream" >/dev/null 2>&1 && break
        sleep 0.01
    done
    jq -e --arg payload "fixture-$name" -s '
        [ .[] | select(.first? != true and .payload? != $payload) ]
    ' "$stream" >"$OUT/events/$name.sequence.json"
    kill "$subscriber" 2>/dev/null || true
    wait "$subscriber" 2>/dev/null || true
    rm -f "$stream"
    trap - RETURN
    printf 'captured %s\n' "$name"
}

switch_to_empty_and_back() {
    run_command workspace __fixture_empty
    run_command workspace 1
}

switch_empty_events() {
    reset_state
    spawn_window fixture-switch
    wait_for_windows 1
    capture_event_sequence workspace-switch-empty switch_to_empty_and_back
}

close_last_window_events() {
    reset_state
    spawn_window fixture-close
    wait_for_windows 1
    run_command workspace __fixture_other
    capture_event_sequence workspace-close-last run_command '[app_id="^fixture-close$"] kill'
}

rename_events() {
    reset_state
    capture_event_sequence workspace-rename run_command 'rename workspace 1 to fixture-renamed'
}

prepare_cross_output_events() {
    reset_state
    run_command create_output
    run_command 'output HEADLESS-1 pos 0 0 mode 800x600'
    run_command 'output HEADLESS-2 pos 800 0 mode 800x600'
    run_command 'workspace __fixture_dst output HEADLESS-2'
}

move_right_empty_destination_events() {
    kill_fixture_windows
    run_command 'focus output HEADLESS-1'
    run_command workspace __fixture_src
    spawn_window fixture-source-1
    spawn_window fixture-source-2
    wait_for_windows 2
    capture_event_sequence workspace-move-right-empty-destination run_command 'move right'
}

move_right_occupied_destination_events() {
    kill_fixture_windows
    run_command workspace __fixture_dst
    spawn_window fixture-destination
    wait_for_windows 1
    run_command 'focus output HEADLESS-1'
    run_command workspace __fixture_src
    spawn_window fixture-source-1
    spawn_window fixture-source-2
    wait_for_windows 3
    capture_event_sequence workspace-move-right-occupied-destination run_command 'move right'
}

move_right_last_source_events() {
    kill_fixture_windows
    run_command workspace __fixture_dst
    spawn_window fixture-destination
    wait_for_windows 1
    run_command 'focus output HEADLESS-1'
    run_command workspace __fixture_last_source
    spawn_window fixture-source-last
    wait_for_windows 2
    capture_event_sequence workspace-move-right-last-source run_command 'move right'
}

cross_output_events() {
    prepare_cross_output_events
    move_right_empty_destination_events
    move_right_occupied_destination_events
    move_right_last_source_events
}

spawn_focused_window() {
    spawn_window fixture-focused
    wait_for_windows 1
}

spawn_unfocused_window() {
    spawn_window fixture-unfocused
    wait_for_windows 2
}

window_map_events() {
    reset_state
    capture_event_sequence window-map-focused spawn_focused_window
    run_command 'no_focus [app_id="^fixture-unfocused$"]'
    capture_event_sequence window-map-unfocused spawn_unfocused_window
    reset_state
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
    if [[ $SCENARIO == inputs || $SCENARIO == inputs-libinput ]]; then
        msg -t get_inputs | jq -S . >"$OUT/$SCENARIO.json"
        return
    fi
    if [[ $SCENARIO == multi-floating ]]; then
        two_floating
        three_floating_raise
        reset_state
        return
    fi
    if [[ $SCENARIO == event-sequences ]]; then
        mkdir -p "$OUT/events"
        switch_empty_events
        close_last_window_events
        rename_events
        reset_state
        return
    fi
    if [[ $SCENARIO == cross-output-events ]]; then
        mkdir -p "$OUT/events"
        cross_output_events
        reset_state
        return
    fi
    if [[ $SCENARIO == window-map-events ]]; then
        mkdir -p "$OUT/events"
        window_map_events
        return
    fi
    if [[ $SCENARIO != all ]]; then
        echo "unknown scenario set: $SCENARIO" >&2
        exit 2
    fi
    rm -f "$OUT"/{*.tree,*.workspaces,*.outputs}.json
    msg -t get_inputs | jq -S . >"$OUT/inputs.json"
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
