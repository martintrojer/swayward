**Sway compatibility**

Swayward speaks sway's IPC protocol on `SWAYSOCK`. `swaymsg`, Waybar, and other
clients can connect without a swayward-specific backend. Queries, the container
tree, workspace and window commands, and the event streams used by common
clients are the strongest parts.

The short version is: try the tool you already use. Unsupported requests and
commands fail explicitly instead of pretending to work. The longer version is
this page.

## Start with the measurements

The public measurements live with the independent
[`sway-ipc-oracle`](https://github.com/martintrojer/sway-ipc-oracle), not in a
second table here:

- [`i3/results/swayward.toml`](https://github.com/martintrojer/sway-ipc-oracle/blob/3d8159e7dbc1b296d52b00e45c12befcdcae605b/i3/results/swayward.toml)
  records the unchanged i3 suite run. Read its pass, skip, and fail figures
  together. A skip usually marks an i3-only premise or a harness boundary, not
  a successful assertion.
- [`sway-ipc/results/swayward.toml`](https://github.com/martintrojer/sway-ipc-oracle/blob/3d8159e7dbc1b296d52b00e45c12befcdcae605b/sway-ipc/results/swayward.toml)
  records the captured sway IPC scenarios as match, mismatch, and not
  applicable.

[`tests/oracle.toml`](https://github.com/martintrojer/swayward/blob/main/tests/oracle.toml)
pins the revision used by this checkout. The [IPC oracle coverage](IPC_ORACLE_COVERAGE.md) page explains what
the scenarios compare and where the faster in-process tests are still blind.
These are measurements, not a compatibility percentage. One passing assertion
does not vouch for the command beside it.

## The wire does not bluff

For an IPC request there are only two honest answers. Swayward either returns
what sway returns, or it returns a structured not-implemented failure. It does
not put swayward-specific content in a valid-looking sway envelope. That reply
would parse, then leave the client to fail somewhere less useful.

`GET_CONFIG` is the clearest example. Sway returns the sway config file
verbatim (`sway/sway/config.c:734-773`, `sway/sway/ipc-server.c:908-917`).
Swayward reads KDL, so it returns `{"success": false}` rather than relabelling
KDL as sway config text. Sway uses the same kind of refusal for `IPC_SYNC`
(`sway/sway/ipc-server.c:919-925`).

The raw request and event inventory lives in
[`tests/sway/compatibility.toml`](https://github.com/martintrojer/swayward/blob/main/tests/sway/compatibility.toml).
`contrib/command-census --check` checks the inventory against the parser and the
figures quoted here. Markdown is not generated from the TOML.

## Requests you can rely on

11 of sway's 15 IPC request types are implemented. The everyday state
queries are in this group:

- `GET_TREE` returns the live nested container tree, including titlebar
  `deco_rect` values. The oracle includes 14 sway 1.12 tree scenarios.
- `GET_WORKSPACES`, `GET_MARKS`, `GET_BINDING_MODES`, and
  `GET_BINDING_STATE` return live compositor state.
- `GET_INPUTS` returns the 12 fields sway emits, with keyboard, pointer, and
  libinput fields present under the same conditions as sway's
  `ipc_json_describe_input` serializer (`sway/sway/ipc-json.c`).
- `GET_SEATS` returns swayward's single seat, its capabilities, focused
  container, and devices.
- `GET_VERSION` returns all 6 sway fields (`sway/sway/ipc-json.c:225-239`).
  Its `variant` is `swayward`; the version number is a swayward release date,
  not a claim to match a sway feature level.
- `SEND_TICK` replies with sway's success object and emits the payload to tick
  subscribers.
- `SYNC` returns sway's exact `{"success": false}` response. Sway does not
  implement this inherited i3 request either, so declining it is compatible
  behaviour (`sway/sway/ipc-server.c:919-925`).
- `GET_BAR_CONFIG` behaves as sway does when no bar is configured: listing bars
  returns `[]`, and asking for an ID returns `No bar with that ID`
  (`sway/sway/ipc-server.c:846-880`).

The partial requests are:

- `RUN_COMMAND` covers most, not all, of sway's runtime command language. The
  next section gives the practical boundary.
- `SUBSCRIBE` accepts 8 of sway's 10 event families. It refuses
  `barconfig_update` and `bar_state_update` because swayward has no managed
  `bar {}` and therefore no corresponding event to send. See
  [Bars](KNOWN_DEVIATIONS.md#bars).
- `GET_OUTPUTS` returns sway 1.12's complete field set and live geometry,
  identity, mode, scale, transform, subpixel layout, current workspace, and
  runtime power state (`sway/ipc-json.c:299-413`). Backend adaptive-sync,
  tearing, HDR, and render-time capability or state do not reach this query
  path, so those fields use disabled defaults rather than made-up backend
  values. Sway derives both `dpms` and `power` from the backend enabled bit
  (`sway/ipc-json.c:340-346`); swayward updates both after `output power` or
  its deprecated `dpms` alias.

The remaining request is `GET_CONFIG`, which is deliberately not implemented
for the reason above. Unknown message numbers receive a structured failure.
Sway instead leaves those clients waiting without a reply; swayward deliberately
diverges because its IPC contract does not permit a request to hang. For the
same reason, a client that half-closes a truncated or oversized frame is
disconnected rather than retained indefinitely as sway retains it.

## Commands: broad, not complete

Sway has one handler system for config directives and IPC commands:
`find_handler_ex` checks the active command table and then the shared table
(`sway/sway/commands.c:162-173`). Settings such as `font`,
`hide_edge_borders`, and `smart_gaps` can therefore be changed with `swaymsg`.

Swayward has two parsers. KDL configuration goes through `swayward-config`, and
runtime commands go through `swayward-ipc`. Most command gaps come from that
seam: the setting exists in KDL, but there is no safe live mutation path for
it yet.

The parser accepts 68 of sway 1.12's 82 unique runtime command names. Of those,
24 command families are complete and 44 are partial. An accepted name means at
least one real form works; it does not mean every option, target, or unit works.

The commonly used forms include:

- focus and movement by direction, workspace, output, mark, or scratchpad;
- workspace switching, naming, assignment, and back-and-forth;
- split, tabbed, stacked, fullscreen, floating, sticky, and border state;
- pixel and percentage resizing;
- marks, criteria, `for_window`, and container swaps;
- runtime keyboard and switch bindings, modes, and several layout settings;
- gaps, output configuration and power, process commands, reload, and exit.

Unsupported syntax returns a `RUN_COMMAND` result with `"success": false`.
Parse failures also include `"parse_error": true`. The parser never turns an
unknown form into a successful no-op.

For the full list, use the hand-maintained data in
[`tests/sway/compatibility.toml`](https://github.com/martintrojer/swayward/blob/main/tests/sway/compatibility.toml).
Each command row includes a realistic probe, its sway source, parser result,
execution classification, state path, and rough cost. The
[`RUN_COMMAND` audit](RUN_COMMAND_AUDIT.md) explains the categories and shows
how to query the census without copying it into another document.

### The larger command gaps

`input` and `seat` are refused as whole namespaces. Sway serves both from tables
shared with its config parser (`sway/sway/commands/input.c` and
`sway/sway/commands/seat.c`). Swayward's input configuration is keyed by device
class rather than by each sway device identifier, and swayward has one seat
created at startup. Accepting either namespace before those state models exist
would be a polite-looking lie.

The `output` namespace is partial. Enable, mode, scale, transform, position,
adaptive-sync, bit depth, modeline, and power have runtime paths. An `*` target
applies to connected outputs; unlike sway, it is not retained as wildcard
configuration for outputs connected later. Other output subcommands fail.

Commands that promise state swayward cannot represent also fail. For example,
`opacity` needs mutable per-container opacity (`sway/commands/opacity.c:9-40`),
`inhibit_idle` needs sway's user-inhibitor policy modes
(`sway/commands/inhibit_idle.c:8-50`, `sway/tree/view.c:281-303`), and
`allow_tearing` needs asynchronous page flips
(`sway/commands/allow_tearing.c:6-25`, `sway/desktop/output.c:254-269`).
`max_render_time` needs per-output and per-view render budgets, which
swayward's frame clock does not have (`sway/commands/max_render_time.c:6-32`,
`sway/desktop/output.c:150-185`, `src/frame_clock.rs`).

Criteria work for native Wayland identity, title, workspace, marks, urgency,
floating state, sandbox metadata, and `xdg_toplevel_tag_v1` tags. Ordinary
Xwayland surfaces arrive through xwayland-satellite as Wayland toplevels, so
separate X11 `class`, `instance`, `window_role`, `window_type`, and XID values
are unavailable. [Known deviations](KNOWN_DEVIATIONS.md) covers that boundary
and the other user-visible differences.

## Events

Subscriptions cover 8 of sway's 10 event families:

- workspace, window, mode, and binding events;
- initial and requested tick events;
- output and shutdown events;
- input hotplug, keymap, and layout events.

The event payloads use sway's names and object shapes. The oracle contains the
captured schemas; focused headless tests exercise live ordering and selected
values. `barconfig_update` and `bar_state_update` are refused at subscription
time because swayward does not manage a bar. A refused subscription is clearer
than one that stays silent forever.

## Clients exercised so far

- **swaywardmsg** ships with swayward and uses the same protocol. It has also
  queried sway successfully, which checks the client half in the other
  direction.
- **swaymsg 1.11** has been used against a nested swayward session. Queries and
  supported commands worked, unsupported commands returned failures, and the
  connection remained usable after an error. This was a manual smoke test.
- **Waybar 0.15.0** has been used without swayward-specific changes. Its
  `sway/workspaces`, `sway/window`, and `sway/mode` modules subscribed and
  refreshed from tree changes. This was also a manual smoke test.

Those checks do not make a claim for every version or every client. If your
client finds a boundary, a short failing example is much more useful than a
compatibility percentage.
