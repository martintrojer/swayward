# Sway IPC fixtures

These files are the oracle for invariant I1: swayward's IPC replies must
preserve sway's JSON schema. The query fixtures and sequence fixtures were last
captured from sway 1.12 on 2026-09-22. `schema-version.json` pins the target tag
and commit. The individual event fixtures and `inputs-libinput.json` remain
sway 1.11 captures as noted below.

## Capture environment

- sway: 1.12, tag commit `88869399f421d9180dd8b6ed0b5a1f4a3585d252`
- host: `linuxpc`, Fedora Linux 44.20260922.0 (Sway Atomic)
- backend: wlroots Wayland backend, nested inside the operator's sway session
- output: `WL-1`, 1270x1408, scale 1, normal transform
- config: solid-color output background, 2 px normal borders, monospace 10
- client: standalone foot terminals with unique `fixture-*` app IDs
- capture command: `contrib/capture-sway-fixtures.sh <nested-SWAYSOCK>`
- multi-floating capture: set `WAYLAND_DISPLAY` to the nested display, then run
  `contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> multi-floating`

Each captured scenario has the raw replies to `get_tree`, `get_workspaces`, and
`get_outputs`, formatted only with `jq -S .` for stable key order. The
`two_floating` and `three_floating_*` scenarios show that `floating_nodes` uses
back-to-front stacking order, while the workspace `focus` array lists the
focused floating window first. `three_floating_after_raise` focuses
`fixture-1`, making the order change observable.

Fourteen of the original fifteen requested scenarios were captured. `urgent`
is absent because foot did not expose an urgency hint after an unfocused
terminal emitted BEL; no substitute fixture was invented.

`inputs.json` was recaptured with the query fixtures from the sway 1.12
Wayland-backend session. It records the backend-neutral pointer and keyboard
values. `inputs-libinput.json` was captured on 2026-09-20 from a capped sway
1.11 `WLR_BACKENDS=headless,libinput` session using the operator's Logitech G703.
It records the real USB IDs and every libinput property that device exposes.
Both files are raw `GET_INPUTS` replies formatted only with `jq`.

## Oracle policy

Never edit these fixtures by hand to make a swayward test pass. That would
invert the oracle. If swayward disagrees with a fixture, swayward is wrong, or
the intentional deviation must be recorded in `docs/data/divergence.toml`.

Only replace fixtures by running the capture script against a separate nested
sway instance. The script rejects a target socket equal to its ambient
`SWAYSOCK`, protecting the operator's live session from its state-changing
scenario setup.

Run `contrib/check-sway-fixture-schema /path/to/sway` with the sway checkout at
tag `1.12`. It reads the target from `schema-version.json`, extracts the output
and native-view field sets from `sway/ipc-json.c`, and compares them with
representative captures. The check fails if the checkout is not the pinned tag
or if a field changes. Update the target and recapture before accepting a later
sway release.

## Event fixtures

The `*.sequence.json` files in `events/` were recaptured from sway 1.12 on
2026-09-22. The individual event fixtures were captured from separate headless
sway 1.11 sessions on 2026-09-13 and 2026-09-14 because the current capture
script does not regenerate them. Those older fixtures remain valid for event
families whose key sets did not change. Window events containing a native view
must also include sway 1.12's unconditional `tag` field; the 1.12 window-map
sequences exercise that schema. `binding.run.json` used
`bindsym Shift+Ctrl+t nop` and injected the chord through sway's
virtual-keyboard protocol.

The `*.sequence.json` files preserve complete ordered event lists from the sway
1.12 installation. `workspace-switch-empty` captures a switch to
an empty workspace and back. `workspace-close-last` captures closing the final
window on an inactive workspace. `workspace-rename` captures a rename. The
`workspace-move-right-*` files capture a focused window moving across two
headless outputs into an empty workspace, into an occupied workspace, and away
from its source workspace's last window. Sway 1.12 emits one `window::move`
event and no workspace event in all three cases. Run
`contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> event-sequences`,
`contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> cross-output-events`, or
`contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> window-map-events` to
replace the corresponding set. The script uses a sway `SEND_TICK` request as
the end-of-stream barrier.

`window-map-focused.sequence.json` and `window-map-unfocused.sequence.json`
were recaptured from the capped sway 1.12 Wayland-backend session. Mapping a
focused window emitted `new`, `title`, then `focus`. A window matched by
`no_focus` emitted only `new` and `title`. The `title` event comes from foot
setting its title after map; the focus distinction is independent of it. Run
`contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> window-map-events` to replace
these captures.

The original capture produced all requested workspace changes: `init`, `empty`,
`focus`, `move`, `rename`, `urgent`, and `reload`. It also produced all requested
window changes: `new`, `close`, `focus`, `title`, `fullscreen_mode`, `move`,
`floating`, `urgent`, and `mark`. Mode fixtures cover `resize` and the return to
`default`. The binding fixture covers sway's complete `change: "run"` keyboard
payload.

The headless conformance test uses `workspace.reload.json`, `window.focus.json`,
and `mode.default.json` because those states match deterministic harness events.
The other files preserve real sway payloads for future event-specific tests.
Never derive or hand-edit an event fixture from swayward output.
