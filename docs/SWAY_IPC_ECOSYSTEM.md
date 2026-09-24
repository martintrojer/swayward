# Sway IPC client coverage

This reference maps the fixed ecosystem clients from the [foundation spec](internal/specs/2026-09-12-swayward-foundation.md#testing-approach) to `src/tests/ipc.rs`. **Tested** means an existing swayward test exercises the listed protocol behavior. **Researched only** means the client source establishes the dependency, but no existing test covers the full behavior. The opt-in probes below now run selected third-party clients against swayward; rows without a probe remain source-level research.

The source review used sway 1.12, Waybar `3672eee`, Quickshell `fae96f1`, autotiling 1.9.4, and DankMaterialShell `eb0b56e`. The links below pin those revisions.

## Client matrix

| Client | IPC use found in upstream source | Existing swayward evidence | Status |
|---|---|---|---|
| `swaymsg` | Exposes `RUN_COMMAND`, `GET_WORKSPACES`, `GET_INPUTS`, `GET_OUTPUTS`, `GET_TREE`, `GET_SEATS`, `GET_MARKS`, `GET_BAR_CONFIG`, `GET_VERSION`, `GET_BINDING_MODES`, `GET_BINDING_STATE`, `GET_CONFIG`, `SEND_TICK`, and `SUBSCRIBE`. `-m` keeps a subscription open. [swaymsg 1.12 source](https://github.com/swaywm/sway/blob/1.12/swaymsg/main.c#L527-L622) | The opt-in `contrib/probe-swaymsg` runs Fedora swaymsg 1.11 with an explicit private socket, parses every exposed request type, checks failures, sends a multi-command, and receives a workspace event in monitor mode. Focused protocol tests cover each request and subscription family. | Tested |
| Quickshell.I3 model | On connection, subscribes to `workspace` and `output`, then requests `GET_WORKSPACES` before `GET_OUTPUTS`. Output events trigger another `GET_OUTPUTS`. Workspace events consume `init`, `focus`, `empty`, `move`, `rename`, `urgent`, and `reload`; reload triggers another `GET_WORKSPACES`. Workspace activation sends `workspace number N`. [controller.cpp](https://github.com/quickshell-mirror/quickshell/blob/fae96f1a5b7f53de9b7e40e5b53c0b7a2e97b1d7/src/x11/i3/ipc/controller.cpp#L31-L308) [workspace.cpp](https://github.com/quickshell-mirror/quickshell/blob/fae96f1a5b7f53de9b7e40e5b53c0b7a2e97b1d7/src/x11/i3/ipc/workspace.cpp#L31-L62) | `live_ipc_descriptions_match_sway_schema_and_values` covers both query shapes. The opt-in `contrib/probe-quickshell-i3` runs the unmodified public model and listener APIs, checks workspace and monitor fields, observes an event, dispatches a workspace switch, and verifies the focused model state. Focused protocol tests cover the remaining event families. | Tested |
| Waybar `sway/workspaces` | Subscribes to `workspace` and `window`; every event triggers `GET_TREE`. Click and scroll paths send workspace-switch commands, criteria-based workspace moves, and `mouse_warping none|container`. [workspaces.cpp](https://github.com/Alexays/Waybar/blob/3672eee03a6abfe883310b92ddbcf3300ae43b2d/src/modules/sway/workspaces.cpp#L88-L110) [command paths](https://github.com/Alexays/Waybar/blob/3672eee03a6abfe883310b92ddbcf3300ae43b2d/src/modules/sway/workspaces.cpp#L458-L573) | The opt-in `contrib/probe-waybar-sway` runs Fedora Waybar 0.15.0, switches workspaces, and reads its rendered focused-workspace label after the event-driven `GET_TREE`. Focused protocol tests cover the command paths. | Tested |
| Waybar `sway/window` | Subscribes to `window` and `workspace`; every event triggers `GET_TREE`. [window.cpp](https://github.com/Alexays/Waybar/blob/3672eee03a6abfe883310b92ddbcf3300ae43b2d/src/modules/sway/window.cpp#L17-L34) | The Waybar probe opens a real Wayland client, changes its title, and reads both rendered titles from the module. `get_tree_hides_windows_on_background_workspaces` and `get_tree_hides_windows_on_inactive_tabs_at_every_depth` cover visibility. | Tested |
| Waybar `sway/mode` | Subscribes to `mode` and reads the event's `change`. [mode.cpp](https://github.com/Alexays/Waybar/blob/3672eee03a6abfe883310b92ddbcf3300ae43b2d/src/modules/sway/mode.cpp#L8-L33) | The Waybar probe reads the rendered `resize` label and confirms that it is hidden after returning to `default`. Focused protocol tests cover the payload and transition. | Tested |
| autotiling 1.9.4 | Uses i3ipc-python. It subscribes to `window` and `mode` by default, runs `GET_TREE` for each event, then sends `splith` or `splitv`. Optional ratios send `resize set width N ppt` or `resize set height N ppt`. [main.py](https://github.com/nwg-piotr/autotiling/blob/2a29a9b37c41f62f49478a160d42c44f7529a61c/autotiling/main.py#L53-L121) [subscriptions](https://github.com/nwg-piotr/autotiling/blob/2a29a9b37c41f62f49478a160d42c44f7529a61c/autotiling/main.py#L151-L209) | The opt-in `contrib/probe-autotiling` runs that pinned source unmodified, opens real clients, and observes its `splith` choice for a wide container and `splitv` choice for a tall container through `GET_TREE`. The i3ipc-python probe separately decodes every library-supported implemented request and all eight supported event types. Optional ratio resizing remains covered only at the command parser. | Tested |
| DankMaterialShell workspace and monitor service | Uses Quickshell.I3 workspaces and monitors. It sends numeric or quoted named workspace commands and `output * dpms off|on`. [CompositorService.qml](https://github.com/AvengeMedia/DankMaterialShell/blob/eb0b56e578aee97ee9605232ca3864d6f4aab4c5/quickshell/Services/CompositorService.qml#L1530-L1592) | The opt-in `contrib/probe-dms-sway` runs Arch dms-shell 1.6.2 with Quickshell 0.3.1, confirms DMS's own Sway detection, and queries DMS's output model through `dms ipc call outputs current`. It drives DPMS through DMS's `lockAndOutputsOff` IPC path, checks DMS's lock model, then observes swayward's power state. DMS exposes no workspace or urgency dispatch or model query through `dms ipc`; those checks remain swayward-side only while DMS is live. | Partial: detection, output model, and DPMS client-side; workspace and urgency swayward-side |
| DankMaterialShell keyboard layout service | Subscribes to `input`, polls `swaymsg -t get_inputs -r`, and sends `input type:keyboard xkb_switch_layout next`. [KeyboardLayoutService.qml](https://github.com/AvengeMedia/DankMaterialShell/blob/eb0b56e578aee97ee9605232ca3864d6f4aab4c5/quickshell/Services/KeyboardLayoutService.qml#L112-L170) | DMS 1.6.2 exposes no keyboard-layout dispatch or model query through `dms ipc`. The probe runs the service's exact command through `swaymsg` while DMS is live, then checks swayward's `GET_INPUTS` state; `input_xkb_switch_layout_changes_get_inputs_and_emits_layout_events` checks the changed index and event payload. | Swayward-side only |
| DankMaterialShell screenshot helper | Runs `swaymsg -t get_workspaces`. [compositor.go](https://github.com/AvengeMedia/DankMaterialShell/blob/eb0b56e578aee97ee9605232ca3864d6f4aab4c5/core/internal/screenshot/compositor.go#L199-L210) | The complete normalized `GET_WORKSPACES` reply is covered by `live_ipc_descriptions_match_sway_schema_and_values`; the external `swaymsg` invocation has not run. | Researched only |

A full-shell row is marked **Tested** only when an opt-in probe makes the real client dispatch the operation and observes the client's resulting model, UI, or logs. Running the client beside a wire-level check is **Swayward-side only**: it does not cover the client's parser, event handling, or model updates.

## `GET_BAR_CONFIG` boundary

Swayward has no `bar {}` configuration. Its no-bar behavior matches sway:

- `GET_BAR_CONFIG` with an empty payload returns `[]`.
- `GET_BAR_CONFIG` with an ID returns `{ "success": false, "error": "No bar with that ID" }`, including sway's byte formatting.
- `SUBSCRIBE` rejects `barconfig_update` and `bar_state_update` with `{ "success": false }`.

`get_bar_config_distinguishes_no_bars_from_an_unknown_id` and `get_bar_config_unknown_id_is_byte_identical_to_sway` test the first two results. Full swaybar configuration and Waybar's `ipc: true` mode remain out of scope; ordinary Waybar Sway modules do not require `GET_BAR_CONFIG`.

## Package availability

The `swayward-dev` container is Fedora 44. These are repository candidates, not installed packages. The Ubuntu column describes the current `ubuntu-24.04` required-test runner.

| Client or dependency | Fedora 44 `swayward-dev` | Ubuntu 24.04 CI | Consequence |
|---|---|---|---|
| `swaymsg` | `sway` 1.11-3.fc44 | `sway` 1.9-1build2 is in Universe | A CI job can install it, but the main Cargo test must not gain this system dependency. |
| Waybar | 0.15.0-2.fc44 | 0.9.24-1build3 is in Universe | Both environments can install it. Versions differ enough that the probe must print and pin its accepted range. |
| Quickshell | 0.2.1 git snapshot `dacfa9d`, Fedora release 5 | No Ubuntu 24.04 package | Use the Fedora development container or a pinned external build, not required Ubuntu CI. |
| i3ipc-python | 2.2.1-20.fc44 | 2.2.1-3 is in Universe | Both environments can install the same upstream release. |
| autotiling | No Fedora 44 package | No Ubuntu 24.04 package | Install pinned 1.9.4 source only in an opt-in probe. |
| DankMaterialShell | No Fedora 44 package | No Ubuntu 24.04 package | The opt-in smoke test uses Arch `dms-shell` 1.6.2-1 with `quickshell` 0.3.1-1 in the dedicated `swayward-dms` distrobox. |

Fedora candidates were measured with `dnf repoquery` inside `swayward-dev`. Ubuntu versions come from the [Ubuntu sway](https://packages.ubuntu.com/sway), [Waybar](https://packages.ubuntu.com/waybar), and [python3-i3ipc](https://packages.ubuntu.com/python3-i3ipc) package indexes.

## Test placement

Keep deterministic protocol and state checks in the required Cargo suite. Put unmodified real-client probes behind `RUN_REAL_CLIENT_TESTS=1` or in `contrib/`; they add distro packages, GUI runtime requirements, and upstream release churn that do not belong in every `cargo test`. Each probe must create an explicit private socket. A probe that launches a nested compositor must use `contrib/dev-run.sh`, including its memory, swap, timeout, socket, and reap safeguards.

Use a manual procedure only for interaction that cannot produce a deterministic machine assertion. None of the protocol slices above needs to start as manual-only: Quickshell, DMS, Waybar, `swaymsg`, and autotiling can all report model state, replies, commands, or logs to an opt-in script.

The typed-library probe should use **i3ipc-python 2.2.1**. Autotiling, one of the foundation clients, directly depends on it, so the probe validates both a library decoder and a real prioritized dependency. `swayipc-rs` is stricter in Rust but is not used by any client in the fixed set; adding it would test a second ecosystem rather than close this task's highest-priority gap.

## swaymsg probe

Fedora swaymsg 1.11 is covered by an opt-in nested-session probe:

```sh
RUN_REAL_CLIENT_TESTS=1 contrib/dev-run.sh --timeout 35 \
    --run "$PWD/contrib/probe-swaymsg"
```

The probe passes `-s` with the private socket on every invocation and checks
all message types exposed by swaymsg 1.11. It parses successful query,
command, and tick replies; checks that `GET_CONFIG` and an unknown bar ID exit
nonzero with structured failures; runs a multi-command; and proves a successful
`subscribe -m` by parsing the workspace event received after a workspace switch.
Set `MUTATE_SWAYMSG_EXPECTATION=1` to confirm that an incorrect event
expectation fails the probe. The command requires the Fedora `sway` 1.11 and
`jq` packages and does not run in the required Cargo suite.

## i3ipc-python probe

Fedora `python3-i3ipc` 2.2.1-20.fc44 is covered by an opt-in nested-session probe:

```sh
RUN_REAL_CLIENT_TESTS=1 contrib/dev-run.sh --timeout 35 \
    --run "$PWD/contrib/probe-i3ipc-python"
```

The probe decodes `RUN_COMMAND`, `GET_WORKSPACES`, `GET_OUTPUTS`, `GET_TREE`,
`GET_MARKS`, the empty `GET_BAR_CONFIG` list, `GET_VERSION`,
`GET_BINDING_MODES`, `GET_INPUTS`, `GET_SEATS`, `SEND_TICK`, and `SUBSCRIBE`.
It accesses attributes on the library's reply, container, and event classes and
observes workspace, output, mode, window, binding, tick, input, and shutdown
events. i3ipc-python 2.2.1 does not define `GET_BINDING_STATE`, so the probe
checks that one reply as decoded JSON rather than claiming typed coverage.
It separately asserts structured failures for `GET_CONFIG`, an unknown bar,
an unsupported subscription, `IPC_SYNC`, and an unknown request. Set
`MUTATE_IPC_FIELD=1` to replace a required workspace attribute and confirm that
the probe fails. The command requires `python3-i3ipc`, `foot`, and `wtype`.
The autotiling loop has a separate probe below.

## autotiling probe

Autotiling 1.9.4 commit `2a29a9b37c41f62f49478a160d42c44f7529a61c`
is covered by an opt-in nested-session probe:

```sh
RUN_REAL_CLIENT_TESTS=1 contrib/dev-run.sh --timeout 45 \
    --run "$PWD/contrib/probe-autotiling"
```

The probe downloads the pinned source archive into the ignored
`.cache/autotiling/` directory, verifies SHA-256
`721754c15541228662cf64789321e8dd87345ac29444a3ebbc1216706d6b1454`, and
runs its `main.py` unmodified. It opens plain `foot` clients and uses
`GET_TREE` to check autotiling's own rule: a wide focused container selects
`splith`, then the resulting tall focused container selects `splitv`. Set
`MUTATE_AUTOTILING_EXPECTATION=1` to make the wide-layout assertion fail. The
command requires `python3-i3ipc` and `foot`; it does not run in the required
Cargo suite.

## Waybar probe

Fedora Waybar 0.15.0 is covered by an opt-in nested-session probe:

```sh
RUN_REAL_CLIENT_TESTS=1 contrib/dev-run.sh --timeout 35 \
    --run "$PWD/contrib/probe-waybar-sway"
```

The probe configures only `sway/workspaces`, `sway/window`, and `sway/mode`.
It reads Waybar's rendered labels through AT-SPI, confirming the focused
workspace after its event-driven `GET_TREE`, a live client title change, and a
mode label appearing and clearing. It requires the Fedora `waybar`, `foot`, and
`python3-pyatspi` packages. Set `MUTATE_WAYBAR_EXPECTATION=1` to confirm that an
incorrect expected label fails the probe. The probe does not enable Waybar's
`ipc` option and does not run in the required Cargo suite.

## DankMaterialShell smoke test

Arch `dms-shell` 1.6.2-1 and Quickshell 0.3.1-1 are covered by a dedicated opt-in environment:

```sh
contrib/dms-arch-container.sh
RUN_REAL_CLIENT_TESTS=1 contrib/dev-run.sh --timeout 50 \
    --config contrib/dms-sway-config.kdl \
    --run "$PWD/contrib/probe-dms-sway"
```

The supplied KDL config must set the keyboard layout to `us,ru`; the default config has only one layout. The setup script creates only the `swayward-dms` distrobox and pins every package passed to `pacman`. The probe starts the packaged, unmodified shell from that Arch container while `contrib/dev-run.sh` retains ownership of the capped compositor. It passes the private socket and nested display explicitly, checks that Arch `swaymsg` reaches the PID encoded in that socket rather than the other live swayward PID, and reaps DMS before returning.

The probe checks DMS's own log for Sway detection and queries its output model through `dms ipc call outputs current`, which must report `winit`. It drives output power through `dms ipc call lock lockAndOutputsOff`, checks DMS's lock model through `lock status`, observes swayward's power state, and unlocks through DMS to restore power. The probe also queries the live `dms ipc` inventory and fails if a workspace, keyboard, compositor, or direct DPMS target appears, so a new client-side path cannot silently remain untested. DMS 1.6.2 exposes none of those targets. Numeric and named workspace switching, urgency, and keyboard layout therefore remain **Swayward-side only**: the probe sends DMS's exact commands through `swaymsg` and checks swayward's resulting state while DMS is live, but cannot assert that DMS dispatched them or updated its model. DMS logs warnings for unavailable login1, NetworkManager, BlueZ, GeoClue, PipeWire, and Polkit services in the containers; none blocks these checks.

## Quickshell probe

Fedora Quickshell 0.2.1 revision `dacfa9de829ac7cb173825f593236bf2c21f637e`
is covered by an opt-in nested-session probe:

```sh
RUN_REAL_CLIENT_TESTS=1 contrib/dev-run.sh --timeout 35 \
    --run "$PWD/contrib/probe-quickshell-i3"
```

The QML config uses only the public `Quickshell.I3` API. It checks workspace
fields, output geometry and scale, observes a workspace event, dispatches
`workspace number 2`, and waits for the model to report workspace 2 focused.
Set `MUTATE_IPC_FIELD=1` with the same command to confirm that the probe fails
when a required workspace field is replaced by a nonexistent one. The command
requires the pinned Fedora package and does not run in the required Cargo suite.
