**Sway compatibility**

<!-- compatibility-census:headline:start -->
**All 15 of sway's IPC requests are answered, and 11 of them behave as sway's
do.** All 8 event families swayward can emit are compared against sway; the
other 2 belong to a bar swayward does not have. Query and event compatibility
is broader than command compatibility; check the command table before assuming
that an accepted command supports every sway form.

The 4 that are not plain "implemented":

- `RUN_COMMAND` executes most of sway's command set but not all of it. This
  is the real gap and the one worth reading about below.
- `SUBSCRIBE` accepts 8 of 10 event families. The 2 it rejects are emitted
  by sway only for a configured `bar {}`, which swayward does not have, so
  there is no event to deliver.
- `GET_OUTPUTS` reports live geometry, scale, transform, identity and mode, but
  conservatively reports unavailable backend capability, rendering and power state.
- `GET_CONFIG` cannot be compliant: sway returns its own config file verbatim
  and swayward's is KDL, so it reports not implemented rather than returning
  something sway-shaped that is not sway.
<!-- compatibility-census:headline:end -->

Each is detailed below with sway source citations.

"Implemented" on this page means the reply was compared against sway 1.12's
implementation at the cited lines. It does not mean every scalar value is
pinned by a fixture; see [IPC oracle coverage](IPC_ORACLE_COVERAGE.md) for
exactly what the automated checks prove and where they are blind. See
[Known deviations](KNOWN_DEVIATIONS.md) for user-visible differences outside
the request matrix.

## Why configuration and commands are separate here

Sway has **one command table for both the config file and IPC**. `find_handler_ex`
consults the config or command table first and then falls back to a shared
`handlers[]` list (`sway/sway/commands.c:162-173`), so in sway there is no real
distinction between a "config directive" and a "runtime command". `font`,
`default_border`, `hide_edge_borders`, `smart_gaps` and the rest are all
`swaymsg`-able, and they take effect immediately: `cmd_font` re-parses the font
and calls `config_update_font_height`, while `hide_edge_borders` and
`smart_gaps` call `arrange_root`.

Swayward separates the two. Configuration is KDL, parsed by `swayward-config`;
runtime commands are a separate parser. They share no table. **That seam, not a
feature decision, is why most of the gaps below exist.** A sway directive that
looks like configuration usually has a working swayward equivalent in KDL; what
it lacks is a way to change it live over IPC.

So the practical question is not "is this supported" but "supported *where*":

<!-- compatibility-census:commands:start -->
- **Accepted.** The parser accepts at least one representative invocation for
  68 of sway's 82 unique runtime command names. Execution-level comparison
  classifies 24 families as complete and 44 as partial.
- **Fail-loud with a stated reason.** 8 command names return a structured
  failure because the underlying state or capability is absent. The accepted
  `urgent` command also rejects its unsupported `allow|deny` policy forms.
- **Deliberate.** 1 command name, `bar`, is unavailable because swayward has no
  managed bar. Sway's 2 ignored client commands are not accepted yet.
- **Unimplemented.** The other 3 rejected names need runtime config, binding,
  criteria, variable, or input/output/seat mutation.
<!-- compatibility-census:commands:end -->

See the [complete `RUN_COMMAND` audit](RUN_COMMAND_AUDIT.md) for every command,
its realistic probe, the matching sway source, the existing swayward state and
apply path, its classification, and its rough cost. `contrib/command-census`
reads that table and checks every recorded parser result. Commands outside the
accepted forms return an error rather than being silently ignored.

## IPC requests

On the wire there are only two honest answers, and every row below is one of
them. A request either behaves exactly as sway's does, or it is **not
implemented** and returns `{"success": false}`. Swayward never returns a
sway-shaped reply carrying swayward-shaped content: that parses, so the client
fails later with nothing to attribute it to. Sway declines `IPC_SYNC` the same
way (`sway/sway/ipc-server.c:919-925`).

`GET_CONFIG` is the worked example. Sway returns the verbatim sway config file,
swayward's config is KDL, and there is no honest conversion, so it is reported
as not implemented rather than approximated.

| Request | Status | Boundary |
|---|---|---|
| `RUN_COMMAND` | Partial | <!-- compatibility-census:request-count:start -->Accepts 68 of sway 1.12's 82 unique runtime command names.<!-- compatibility-census:request-count:end --> Unsupported syntax returns a sway-shaped failure array. See the [complete command audit](RUN_COMMAND_AUDIT.md). |
| `GET_WORKSPACES` | Implemented | Returns live global workspace identities. Fixture and focused tests cover the top-level shape and selected values. |
| `SUBSCRIBE` | Partial | <!-- compatibility-census:subscribe-count:start -->Accepts 8 of sway's 10 families<!-- compatibility-census:subscribe-count:end -->: `workspace`, `output`, `mode`, `shutdown`, `window`, `binding`, `tick`, and `input`. Rejects `barconfig_update` and `bar_state_update`. Sway emits both only for a configured `bar {}`, which swayward does not have, so there is no event to deliver; the subscription is refused rather than accepted and left permanently silent. See [Bars](KNOWN_DEVIATIONS.md#bars). |
| `GET_OUTPUTS` | Partial | Returns sway 1.12's complete field set and live geometry, scale, transform, subpixel layout, identity, current workspace, current mode, and runtime power state (`sway/ipc-json.c:299-413`). Sway reports both `dpms` and `power` from the backend enabled bit (`sway/ipc-json.c:340-346`), and swayward reports both from the power state changed by `output power` or its deprecated `dpms` alias. The schema is captured from the exact 1.12 tag, and `contrib/check-sway-fixture-schema` compares fixture keys with that source. The serializer sees only active layout outputs, so `active` is always true and `primary` is always false. Swayward has no scale-filter setting, so `scale_filter` is `nearest`. Adaptive-sync, tearing, HDR, and render-time capability/state do not reach this query path, so those fields conservatively report their disabled defaults rather than claiming the captured backend's values. |
| `GET_TREE` | Implemented | Returns the live nested container tree, including titlebar `deco_rect` values. <!-- compatibility-census:tree-fixtures:start -->Fourteen sway 1.12 fixtures cover its schema and selected semantics.<!-- compatibility-census:tree-fixtures:end --> |
| `GET_MARKS` | Implemented | Returns marks created and removed through commands. |
| `GET_BAR_CONFIG` | Implemented | Empty payload returns `[]`; a requested ID returns sway's exact `{ "success": false, "error": "No bar with that ID" }`. Both are byte-identical to what sway replies when no bar is configured (`sway/sway/ipc-server.c:846-880`), so the request is fully compliant. Swayward has no `bar {}` block, so the configured-bar list is always empty; that is the deliberate part, and it lives in the config, not on the wire. See [Bars](KNOWN_DEVIATIONS.md#bars). |
| `GET_VERSION` | Implemented | Returns <!-- compatibility-census:version-fields:start -->all six fields sway returns<!-- compatibility-census:version-fields:end --> (`sway/sway/ipc-json.c:225-239`): `human_readable`, `variant`, `major`, `minor`, `patch`, `loaded_config_file_name`. `variant` is `swayward`, which is the field sway provides for exactly this purpose and is how a client distinguishes the two. The version numbers are a date (see [versioning](UPSTREAM.md#versioning)), not a sway feature level. No sway fixture pins the individual values. |
| `GET_BINDING_MODES` | Implemented | Returns `default` followed by configured mode names. |
| `GET_CONFIG` | Not implemented | Returns `{"success": false}`. Sway's contract is the verbatim text of the sway config file (`sway/sway/config.c:734-773`, `sway/sway/ipc-server.c:908-917`), and swayward's config is KDL, so there is nothing sway-shaped to return. Serving KDL in sway's envelope would be well-formed and wrong, breaking a client with no error to attribute it to. Sway answers a request it declines the same way (`sway/sway/ipc-server.c:919-925`, IPC_SYNC). |
| `SEND_TICK` | Implemented | Replies with sway's success object and emits the payload to `tick` subscribers. |
| `SYNC` | Implemented | Returns `{"success": false}`, byte-identical to sway. Sway decided not to support this i3 request and replies the same way rather than closing the socket (`sway/sway/ipc-server.c:919-925`). Declining it *is* the compliant behaviour. |
| `GET_BINDING_STATE` | Implemented | Returns `{"name": mode}`. The green `311-get-binding-modes.t` file exercises message type 12 at startup and after a mode change. |
| `GET_INPUTS` | Implemented | Returns live input devices. <!-- compatibility-census:input-fields:start -->All 12 fields sway emits are present and conditioned as sway conditions them<!-- compatibility-census:input-fields:end --> (`sway/sway/ipc-json.c`, `ipc_json_describe_input`): `repeat_delay`, `repeat_rate` and the <!-- compatibility-census:xkb-fields:start -->three `xkb_*` fields<!-- compatibility-census:xkb-fields:end --> on keyboards, `scroll_factor` on pointers only, and a `libinput` sub-object whose `send_events` is unconditional while `tap*` and `accel_*` appear only when the device reports the capability. Verified against a live session, not only the fixture. |
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

| Family | Supported forms and limits |
|---|---|
| Focus | Bare `focus`; directions; `parent`, `child`, `next`, and `prev`; `next|prev sibling`; `floating`, `tiling`, and `mode_toggle`; `focus output <direction|name>`; and criteria-targeted `focus workspace`. |
| Move | Directional moves with an omitted or pixel distance; floating `move position` by coordinates, center, or pointer; moves to a workspace, output, mark, or scratchpad; and workspace moves to an output. Directional distances other than pixels return a failure. Absolute positions do not accept percentage points. |
| Workspace | Switch by name or number, including `next`, `prev`, `next_on_output`, `prev_on_output`, `back_and_forth`, and `current`; assign a workspace to an output; and rename a workspace. |
| Layout | `layout splith|splitv|tabbed|stacked|stacking|default`; default, split, all, and explicit-list toggle cycles; and `split h|v|t|toggle`. `split none` returns a failure. |
| Window state | Workspace and global `fullscreen enable|disable|toggle`; `floating enable|disable|toggle`; `urgent enable|disable|toggle` with sway's boolean aliases; `border`; `sticky`; and `title_format`. `urgent allow|deny` remains fail-loud because swayward does not store the per-window permission that sway checks before accepting a client urgency request (`sway/commands/urgent.c:9-31`; `sway/desktop/xwayland.c:766-773`). |
| Scratchpad | `move scratchpad` and `scratchpad show` |
| Resize | Grow or shrink an axis in pixels or percentage points, with an optional fallback amount; set width, height, or both. |
| Layout settings | `focus_wrapping`, `workspace_layout`, `default_orientation` (and its `orientation` alias), `hide_edge_borders`, `smart_borders`, `focus_follows_mouse`, `workspace_auto_back_and_forth`, `floating_minimum_size`, `floating_maximum_size`, `font`, `titlebar_padding`, `titlebar_border_thickness`, `mouse_warping`, `xwayland`, `popup_during_fullscreen`, `floating_modifier`, and `default_border` with its `default_floating_border`, `new_window` and `new_float` spellings, plus the deprecated `force_focus_wrapping`. At runtime, `mouse_warping output|container|none` stores sway's three-state policy. KDL has no `mouse-warping` node, so the translator approximates enabled modes with `warp-mouse-to-focus`; `reload` resets the runtime policy to `none` and applies that inherited centering option. `xwayland` is accepted but refuses a change, because the mode is fixed at launch in sway too. Sway serves these from the same table as its config file, so they are live commands there; swayward stores them in KDL and re-applies the config after each change. Accepted values and error strings follow sway's own command files, including `hide_edge_borders smart` folding into the smart-border toggle. `reload` re-reads the file and discards these runtime changes, as in sway. Output configuration is the exception: swayward keeps a transient output change across a reload that did not edit the output settings, where sway discards it. See [Reload and transient output configuration](KNOWN_DEVIATIONS.md#reload-and-transient-output-configuration). |
| Gaps | Change inner or per-side outer gaps on the current workspace or all workspaces with `set`, `plus`, `minus`, or `toggle`. |
| Runtime bindings | `bindsym`, `unbindsym`, `bindcode`, and `unbindcode` mutate the default or active-mode binding vector that keyboard dispatch reads on every event. Equal key/flag/device bindings overwrite in place; unbinding a missing binding fails. `--release`, `--locked`, `--inhibited`, `--no-repeat`, `--no-warn`, and `--input-device=` are supported for keyboard bindings. `bindswitch` and `unbindswitch` use a runtime mode-keyed command table checked before KDL's narrower spawn-only switch events; `on`, `off`, `toggle`, `--locked`, and `--no-warn` work. Reload discards all runtime binds, as sway does. Mouse-region bindings, XKB `GroupN`, `--to-code`, criteria-targeted forms, switch `--reload`, and replacing a KDL switch-event binding remain fail-loud. Gesture binds remain unavailable because gesture events have no sway command-binding path. |
| Process and session | `exec`, `exec_always`, `exit`, `kill`, `reload`, `mode <name>`, `nop`, and headless-only `create_output` with sway's 1920×1080 default. The tty and single-window winit backends cannot add outputs and return sway's `Can only create outputs for Wayland, X11 or headless backends` failure without changing output state (`sway/commands/create_output.c:12-52`). |
| Marks, rules, and swap | `mark`, `unmark`, `for_window`, and `swap container with id|con_id|mark`. Native Wayland windows have no X11 ID for the `id` form. |
| Output power | `output <name> power on|off|toggle` and the deprecated `dpms` alias. `output * power|dpms on|off` fans out over connected outputs; sway rejects wildcard toggle (`sway/commands/output/power.c:15-31`). Both spellings update `GET_OUTPUTS` and emit `output::unspecified`, as sway does after applying output state (`sway/ipc-json.c:340-346`; `sway/desktop/output.c:397-399`; `sway/ipc-server.c:510-518`). Reload resets runtime power state because sway rebuilds and replaces its output-config list (`sway/commands/reload.c:28-34`; `sway/config.c:136-141,220-226`). Replug retains the state because sway stores the command in that list and reapplies matching entries to a new output (`sway/config/output.c:241-294,703-738`). Idle wake does not mutate output configuration; idle activity only notifies wlroots (`sway/input/seat.c:106-112`), so a user-powered-off output remains off. |

Criteria parsing accepts sway-style selectors, including `app_id`, `title`,
`workspace`, `con_id`, `con_mark`, urgency, floating or tiling state, sandbox
metadata, and the case-sensitive tag set through `xdg_toplevel_tag_v1`.
Swayward cannot populate separate X11 `class`,
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

The `input` and `seat` namespaces are rejected as a whole. Sway serves them
from command tables shared with its config file (`sway/sway/commands/input.c`;
`sway/sway/commands/seat.c`). Swayward reads their equivalent settings from KDL
and has no general runtime mutation path, so each returns a failure rather than
a silent no-op.

The `output` namespace is partial. Core enable, mode, scale, transform,
position, adaptive-sync, bit-depth, modeline, and power commands have runtime
paths. The `*` target fans out over currently connected outputs rather than
storing sway's wildcard config for future outputs. Other output subcommands
remain fail-loud.

The two wholly rejected namespaces are not equally far away, and the distance
is in the configuration model rather than in the command parsers.

- **`input`** needs a configuration model change first. Sway addresses a device
  by identifier or by `type:<class>`; swayward's input settings are keyed by
  device class only (`swayward-config/src/input.rs:12-26`), with no per-device
  map. Swayward computes sway-shaped identifiers for binds and reports them
  through `GET_INPUTS`, so it can name a device it cannot yet configure
  individually. Per-device settings would change the KDL schema, not just add a
  command.
- **`seat`** has the least to act on. Swayward runs a single seat created at
  startup (`src/swayward.rs:2517-2518`) and has no seat configuration block, so
  `attach`, `fallback`, and `keyboard_grouping` have no second seat to address.
  `hide_cursor` and `xcursor_theme` have KDL equivalents under `cursor`.

## Events

Swayward supports subscriptions for these event families:

| Event | Status | Evidence |
|---|---|---|
| Workspace | Emitted | Emits every change value sway emits: `init`, `focus`, `empty`, `rename`, `move`, `urgent`, and `reload`. `move` and `urgent` are each compared with a sway 1.11 fixture (`workspace_move_event_matches_sway_shape`, `workspace_urgency_event_matches_sway_shape`). A whole-layout change with no sway equivalent, such as a config reload that rebuilds workspaces, emits `reload`, which is the value sway uses for the same situation (`sway/sway/commands/reload.c`). |
| Window | Emitted | A headless test compares a live `focus` event with a sway fixture. Sway 1.12 window-map sequences pin the current native-view schema, including `tag`; the remaining individual event fixtures are 1.11 captures and most are not emitted one by one in tests. |
| Mode | Emitted | A headless subscriber switches to a configured mode and compares the event with sway 1.11's `resize` fixture. |
| Binding | Emitted | Real keyboard and pointer bindings emit `binding::run`. A headless test compares the payload with a sway 1.11 fixture. |
| Tick | Emitted | A tick subscription first receives `{"first":true,"payload":""}` after the successful subscription reply. `SEND_TICK` then emits the supplied payload with `first:false`, matching sway's ordering and flags. |
| Output | Emitted | Output configuration changes emit sway's exact `{"change":"unspecified"}` payload. |
| Shutdown | Emitted | SIGINT, SIGTERM, SIGHUP, confirmed quit actions, and a nested-window close emit `{"change":"exit"}` before stopping the event loop. |
| Input | Emitted | Device hotplug emits `added` and `removed`; keymap reloads emit `xkb_keymap`; layout switches emit `xkb_layout`. Each event reuses the exact device object returned by `GET_INPUTS`. Swayward does not emit `libinput_config`: config reload is its only runtime libinput mutation path, and its backend-neutral input model cannot determine whether applying a setting changed a given libinput device (`sway/input/libinput.c:200-348`). |
| Bar configuration and state | Deliberate deviation | `SUBSCRIBE` rejects `barconfig_update` and `bar_state_update`; swayward does not manage a bar. See [Bars](KNOWN_DEVIATIONS.md#bars). |

## Verified clients

- **swaywardmsg:** ships with swayward and speaks the same protocol. Tested
  against sway 1.11 as well, where `get_version`, `get_workspaces`, and
  `get_tree` all returned correct replies, which confirms the client half of
  the protocol in both directions.
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
