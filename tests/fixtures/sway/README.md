# Sway IPC fixtures

These files were captured from a real nested sway session on 2026-09-12. They
are the oracle for invariant I1: swayward's IPC replies must preserve sway's
JSON schema.

## Capture environment

- sway: 1.11 (`variant: sway`, version `1.11.0`)
- host: `linuxpc`, Fedora Linux 44.20260912.0 (Sway Atomic)
- backend: wlroots Wayland backend, nested inside the operator's sway session
- output: `WL-1`, 1270x1408, scale 1, normal transform
- config: solid-color output background, 2 px normal borders, monospace 10
- client: standalone foot terminals with unique `fixture-*` app IDs
- capture command: `contrib/capture-sway-fixtures.sh <nested-SWAYSOCK>`
- multi-floating capture: set `WAYLAND_DISPLAY` to the nested display, then run
  `contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> multi-floating`

Each captured scenario has the raw replies to `get_tree`, `get_workspaces`, and
`get_outputs`, formatted only with `jq -S .` for stable key order. The
`two_floating` and `three_floating_*` scenarios were captured on 2026-09-13 from
the same sway 1.11 installation. They show that `floating_nodes` uses
back-to-front stacking order, while the workspace `focus` array lists the
focused floating window first. `three_floating_after_raise` focuses
`fixture-1`, making the order change observable.

Fourteen of the original fifteen requested scenarios were captured. `urgent`
is absent because foot did not expose an urgency hint after an unfocused
terminal emitted BEL; no substitute fixture was invented.

## Oracle policy

Never edit these fixtures by hand to make a swayward test pass. That would
invert the oracle. If swayward disagrees with a fixture, swayward is wrong, or
the intentional deviation must be recorded in `docs/DIVERGENCE.md`.

Only replace fixtures by running the capture script against a separate nested
sway instance. The script rejects a target socket equal to its ambient
`SWAYSOCK`, protecting the operator's live session from its state-changing
scenario setup.

## Event fixtures

The files in `events/` were captured from separate headless sway 1.11 sessions
on 2026-09-13 and 2026-09-14. Read-only `swaymsg -t subscribe -m` clients
recorded raw workspace, window, mode, and binding event streams. The JSON files
contain the first captured payload for each `change` value, formatted only with
`jq -S .`. `binding.run.json` used `bindsym Shift+Ctrl+t nop` and injected the
chord through sway's virtual-keyboard protocol.

The `*.sequence.json` files preserve complete ordered workspace event lists from
the same sway 1.11 installation. `workspace-switch-empty` captures a switch to
an empty workspace and back. `workspace-close-last` captures closing the final
window on an inactive workspace. `workspace-rename` captures a rename. The
`workspace-move-right-*` files capture a focused window moving across two
headless outputs into an empty workspace, into an occupied workspace, and away
from its source workspace's last window. Real sway 1.11 emits no workspace
events in all three cases; each file is therefore an empty JSON array. Run
`contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> event-sequences` or
`contrib/capture-sway-fixtures.sh <nested-SWAYSOCK> cross-output-events` to
replace the corresponding set. The script uses a sway `SEND_TICK` request as
the end-of-stream barrier.

The capture produced all requested workspace changes: `init`, `empty`, `focus`,
`move`, `rename`, `urgent`, and `reload`. It also produced all requested window
changes: `new`, `close`, `focus`, `title`, `fullscreen_mode`, `move`, `floating`,
`urgent`, and `mark`. Mode fixtures cover `resize` and the return to `default`.
The binding fixture covers sway's complete `change: "run"` keyboard payload.

The headless conformance test uses `workspace.reload.json`, `window.focus.json`,
and `mode.default.json` because those states match deterministic harness events.
The other files preserve real sway payloads for future event-specific tests.
Never derive or hand-edit an event fixture from swayward output.
