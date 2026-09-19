# Sway compatibility

This reference lists the sway interfaces that swayward implements. “Implemented”
means that the request has a handler. It does not mean that every value has been
proved equivalent to sway.

See [IPC oracle coverage](IPC_ORACLE_COVERAGE.md) for the exact automated checks
and their known blind spots. See [Known deviations](KNOWN_DEVIATIONS.md) for
user-visible differences outside the request matrix.

## IPC requests

| Request | Status | Boundary |
|---|---|---|
| `RUN_COMMAND` | Partial | Executes the command families listed below. Unsupported syntax returns a sway-shaped failure array. |
| `GET_WORKSPACES` | Implemented | Returns live global workspace identities. Fixture and focused tests cover the top-level shape and selected values. |
| `SUBSCRIBE` | Partial | Accepts 8 of sway's 10 families: `workspace`, `output`, `mode`, `shutdown`, `window`, `binding`, `tick`, and `input`. It rejects `barconfig_update` and `bar_state_update`, which belong to the deliberate [bar deviation](KNOWN_DEVIATIONS.md#bars). |
| `GET_OUTPUTS` | Implemented | Returns live outputs. Fixture and focused tests cover the top-level shape and selected values. |
| `GET_TREE` | Implemented | Returns the live nested container tree, including titlebar `deco_rect` values. Fourteen sway 1.11 fixtures cover its schema and selected semantics. |
| `GET_MARKS` | Implemented | Returns marks created and removed through commands. |
| `GET_BAR_CONFIG` | Deliberate deviation | Empty payload returns `[]`; a requested ID returns sway's `No bar with that ID` error. Swayward has no `bar {}` block and does not launch swaybar. |
| `GET_VERSION` | Implemented | Returns sway's response fields with `variant` set to `swayward`. Most field values do not have a sway fixture comparison. |
| `GET_BINDING_MODES` | Implemented | Returns `default` followed by configured mode names. |
| `GET_CONFIG` | Implemented | Returns the raw top-level KDL in sway's single-field schema. |
| `SEND_TICK` | Supported | Replies with sway's success object and emits the payload to `tick` subscribers. |
| `GET_BINDING_STATE` | Implemented | Returns `{"name": mode}`. The green `311-get-binding-modes.t` file exercises message type 12 at startup and after a mode change. |
| `GET_INPUTS` | Implemented | Returns live physical input devices. Keyboard entries include repeat and XKB layout state; unsupported libinput-only metadata is omitted. |
| `GET_SEATS` | Implemented | Returns swayward's single seat, capabilities, focused container ID, and devices. |

Unknown message numbers are rejected by the wire decoder. The automated suite
does not enumerate every unsupported request. In particular, it does not assert
every unsupported request's exact error response.

## Runtime commands

The same parser serves `swaymsg` and KDL bindings such as:

```kdl
binds {
    Mod+H { command "focus left"; }
}
```

The parser and executor support these command families:

| Family | Supported forms and limits |
|---|---|
| Focus | Bare `focus`; directions; `parent`, `child`, `next`, and `prev`; `next|prev sibling`; `floating`, `tiling`, and `mode_toggle`; `focus output <direction|name>`; and criteria-targeted `focus workspace`. |
| Move | Directional moves with an omitted or pixel distance; floating `move position` by coordinates, center, or pointer; moves to a workspace, output, mark, or scratchpad; and workspace moves to an output. Directional distances other than pixels return a failure. Absolute positions do not accept percentage points. |
| Workspace | Switch by name or number, including `next`, `prev`, `next_on_output`, `prev_on_output`, `back_and_forth`, and `current`; assign a workspace to an output; and rename a workspace. |
| Layout | `layout splith|splitv|tabbed|stacked|stacking|default`; default, split, all, and explicit-list toggle cycles; and `split h|v|t|toggle`. `split none` returns a failure. |
| Window state | Workspace and global `fullscreen enable|disable|toggle`; `floating enable|disable|toggle`; `urgent enable|disable|toggle` with sway's boolean aliases; `border`; `sticky`; and `title_format`. `urgent allow|deny` remains fail-loud because swayward does not store the per-window permission that sway checks before accepting a client urgency request (`sway/commands/urgent.c:9-31`; `sway/desktop/xwayland.c:766-773`). |
| Scratchpad | `move scratchpad` and `scratchpad show` |
| Resize | Grow or shrink an axis in pixels or percentage points, with an optional fallback amount; set width, height, or both. |
| Gaps | Change inner or per-side outer gaps on the current workspace or all workspaces with `set`, `plus`, `minus`, or `toggle`. |
| Process and session | `exec`, `exec_always`, `exit`, `kill`, `reload`, `mode <name>`, `nop`, and headless-only `create_output` with sway's 1920×1080 default. Other backends return sway's `Expected a multi backend` failure (`sway/commands/create_output.c:12-52`). |
| Marks, rules, and swap | `mark`, `unmark`, `for_window`, and `swap container with id|con_id|mark`. Native Wayland windows have no X11 ID for the `id` form. |

Criteria parsing accepts sway-style selectors, including `app_id`, `title`,
`workspace`, `con_id`, `con_mark`, Xwayland identity fields, urgency, and
floating or tiling state. Swayward cannot populate separate X11 `class`,
`instance`, `window_role`, or `window_type` values for the ordinary Wayland
surfaces supplied by xwayland-satellite. Criteria-targeted execution supports
focus; move by direction, position, workspace, output, mark, or scratchpad; workspace moves to
an output; marks; swap; fullscreen; sticky; title format; borders; floating;
kill; resize; container layout; and `nop`. Other commands with criteria return a
failure instead of running against the wrong target.

Commands outside these forms return a `RUN_COMMAND` result with
`"success":false`. Parse failures also include `"parse_error":true`.
`opacity` remains fail-loud: swayward can render rule-derived per-window
opacity, but it has no mutable per-container opacity state for sway's
`set|plus|minus` command (`sway/commands/opacity.c:9-40`). `inhibit_idle` also
remains fail-loud. Swayward honors application-created idle inhibitors, but it
has no user-inhibitor object or the `focus`, `fullscreen`, `open`, and `visible`
policy modes required by sway (`sway/commands/inhibit_idle.c:8-50`;
`sway/tree/view.c:281-303`).

`allow_tearing` remains fail-loud because swayward's DRM renderer does not
request asynchronous page flips; accepting it would promise immediate
presentation that the compositor cannot provide (`sway/commands/allow_tearing.c:6-25`;
`sway/desktop/output.c:254-269`). `max_render_time` also remains fail-loud:
sway delays rendering by the output and focused view budgets, while swayward's
frame clock has no per-view deadline (`sway/commands/max_render_time.c:6-32`;
`sway/desktop/output.c:150-185`; `src/frame_clock.rs`). The exact missing-argument
error remains `Missing max render time argument.`

Swayward implements the top-level `shortcuts_inhibitor enable|disable` command.
Each view stores a separate future-request policy. `enable` changes only that
policy; `disable` also deactivates the view's current inhibitor. New protocol
requests activate for the default and enabled policies and remain inactive for
the disabled policy, matching sway (`sway/commands/shortcuts_inhibitor.c:10-49`;
`sway/input/input-manager.c:288-345`). The separate `seat <name>
shortcuts_inhibitor activate|deactivate|toggle` namespace remains unsupported.

## Events

Swayward supports subscriptions for these event families:

| Event | Status | Evidence |
|---|---|---|
| Workspace | Partial | Emits `init`, `focus`, `empty`, `rename`, and `reload`. Workspace moves and urgency changes currently emit the generic `reload` change instead of sway's `move` and `urgent` changes. |
| Window | Emitted | A headless test compares a live `focus` event with a sway 1.11 fixture. Fixtures preserve all documented window change values, but most are not emitted one by one in tests. |
| Mode | Emitted | A headless subscriber switches to a configured mode and compares the event with sway 1.11's `resize` fixture. |
| Binding | Emitted | Real keyboard and pointer bindings emit `binding::run`. A headless test compares the payload with a sway 1.11 fixture. |
| Tick | Emitted | A tick subscription first receives `{"first":true,"payload":""}` after the successful subscription reply. `SEND_TICK` then emits the supplied payload with `first:false`, matching sway's ordering and flags. |
| Output | Emitted | Output configuration changes emit sway's exact `{"change":"unspecified"}` payload. |
| Shutdown | Emitted | SIGINT, SIGTERM, SIGHUP, confirmed quit actions, and a nested-window close emit `{"change":"exit"}` before stopping the event loop. |
| Input | Emitted | Device hotplug emits `added` and `removed`; keymap reloads emit `xkb_keymap`; layout switches emit `xkb_layout`. Each event reuses the exact device object returned by `GET_INPUTS`. Swayward does not emit `libinput_config`: config reload is its only runtime libinput mutation path, and its backend-neutral input model cannot determine whether applying a setting changed a given libinput device (`sway/input/libinput.c:200-348`). |
| Bar configuration and state | Deliberate deviation | `SUBSCRIBE` rejects `barconfig_update` and `bar_state_update`; swayward does not manage a bar. See [Bars](KNOWN_DEVIATIONS.md#bars). |

## Verified clients

- **swaymsg 1.11:** tested manually against a nested compositor. Tree and state
  queries returned replies, supported commands ran, unsupported commands returned
  failures, and the connection remained usable after an error. This is a manual
  smoke test, not an automated client test.
- **waybar 0.15.0:** tested manually without swayward-specific changes. Its
  `sway/workspaces`, `sway/window`, and `sway/mode` modules subscribed without IPC
  errors. The bar rendered at 1905 by 34 pixels, and workspace or window changes
  caused fresh tree queries. This is a manual smoke test, not an automated waybar
  test.

No compatibility claim is made here for other clients.
