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
| `SUBSCRIBE` | Partial | Accepts only `workspace`, `window`, and `mode`. Swayward emits events for all three families. |
| `GET_OUTPUTS` | Implemented | Returns live outputs. Fixture and focused tests cover the top-level shape and selected values. |
| `GET_TREE` | Implemented | Returns the live nested container tree, including titlebar `deco_rect` values. Fourteen sway 1.11 fixtures cover its schema and selected semantics. |
| `GET_MARKS` | Implemented | Returns marks created and removed through commands. |
| `GET_BAR_CONFIG` | Stubbed | Returns `[]`. Swayward has no `bar {}` block and does not launch swaybar. |
| `GET_VERSION` | Implemented | Returns sway's response fields with `variant` set to `swayward`. Most field values do not have a sway fixture comparison. |
| `GET_BINDING_MODES` | Implemented | Returns `default` followed by configured mode names. |
| `GET_CONFIG` | Unsupported | Returns `{"success":false,"error":"not implemented"}`. |
| `SEND_TICK` | Unsupported | Returns `{"success":false,"error":"not implemented"}`. |
| `GET_BINDING_STATE` | Unsupported | Returns `{"success":false,"error":"not implemented"}`. |
| `GET_INPUTS` | Unsupported | Returns `{"success":false,"error":"not implemented"}`. |
| `GET_SEATS` | Unsupported | Returns `{"success":false,"error":"not implemented"}`. |

Unknown message numbers are rejected by the wire decoder. The automated suite
does not enumerate every unsupported request. In particular, it does not assert
the current error response for `GET_CONFIG`, `GET_INPUTS`, or `GET_SEATS`.

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
| Focus | `focus left|right|up|down|parent|child|next|prev` |
| Move | `move left|right|up|down`; an omitted distance or `10 px` works. Other explicit distances return a failure. |
| Workspace | Switch to named or numbered workspaces; use `next`, `prev`, `next_on_output`, or `prev_on_output`; move a window to a workspace; assign a workspace to an output. |
| Layout | `layout splith|splitv|tabbed|stacked|toggle split`; `split h|v|toggle`. `split none` returns a failure. |
| Window state | Local `fullscreen enable|disable|toggle` and `floating enable|disable|toggle`. Global fullscreen returns a failure. |
| Scratchpad | `move scratchpad` and `scratchpad show` |
| Resize | Grow or shrink width or height in pixels or percentage points. |
| Process and session | `exec`, `exec_always`, `kill`, `reload`, `mode <name>`, and `nop` |
| Marks and rules | `mark`, `unmark`, and `for_window` |

Criteria parsing supports sway-style selectors, including `app_id`, `title`,
`workspace`, `con_id`, `con_mark`, Xwayland identity fields, urgency, and
floating or tiling state. Targeted execution is narrower: marks, fullscreen,
floating state, container layout, and `nop` are implemented. Other commands with
criteria return a failure instead of running against the wrong target.

Commands outside these forms return a `RUN_COMMAND` result with
`"success":false`. Parse failures also include `"parse_error":true`.

## Events

Swayward supports subscriptions for these event families:

| Event | Status | Evidence |
|---|---|---|
| Workspace | Emitted | A headless test compares a live `reload` event with a sway 1.11 fixture. Fixtures preserve all documented workspace change values, but most are not emitted one by one in tests. |
| Window | Emitted | A headless test compares a live `focus` event with a sway 1.11 fixture. Fixtures preserve all documented window change values, but most are not emitted one by one in tests. |
| Mode | Emitted | A headless subscriber switches to a configured mode and compares the event with sway 1.11's `resize` fixture. |
| Other sway event families | Unsupported | No event production or compatibility claim. |

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
