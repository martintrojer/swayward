# IPC oracle coverage

This reference records what `src/tests/ipc.rs` verifies against sway and where it can still accept an incompatible reply. Treat each entry as narrow: a caught mutation proves only the listed behavior.

## Empirical method

For each row, one emitter behavior was changed, `cargo test -p swayward --lib tests::ipc` was run, and the change was reverted. A nonzero result means at least one IPC test caught the mutation. An exit code of 0 means every IPC test accepted it.

## Coverage matrix

| Surface | Mutation | Result | Boundary |
|---|---|---:|---|
| `GET_TREE` node schema and values | Change a node key set, JSON value type, leaf name/layout/orientation, focus/percent, or rectangle beyond 10 px | Caught | The live tests read `GET_TREE` through the Unix socket and compare values with sway 1.11 captures; shape checks remain supplemental. Dynamic IDs are mapped by tree position. Percentages use an absolute tolerance below 1e-9. Rect coordinates and dimensions allow 10 logical pixels for host-font titlebar differences, which still rejects collapsed or unit-size geometry. Workspace layout/representation is covered separately because sway stores a workspace's pending split while swayward stores the root tree layout. |
| `GET_TREE` focus | Change focus MRU contents or order | Caught | `live_ipc_focus_matches_sway_mru_arrays` maps dynamic IDs and compares each node's focus array. |
| `GET_TREE` percent | Change parent shares, nullability, or a sibling sum | Caught | `live_ipc_percent_matches_sway_parent_shares` compares fixture values and sums. |
| `GET_WORKSPACES` | Return `[]` | Caught: 18 passed, 1 failed | The live wire test checks one entry and its `num`, `name`, and `output`. Fixture comparison also checks top-level length and element shape. Other scalar values remain shape-only unless a focused test covers them. |
| `GET_OUTPUTS` | Return `[]` | Caught | The live wire test checks one entry and its `name`. Fixture comparison also checks top-level length and element shape. Other scalar values remain shape-only unless a focused test covers them. |
| `GET_MARKS` | Return `[]` after creating a mark | Caught: 17 passed, 2 failed | Tests compare the complete one-mark reply and an empty reply after unmark. There is no sway-captured `GET_MARKS` fixture. |
| `RUN_COMMAND` | Replace every failure outcome with `[{"success":true}]` | Caught: 23 passed, 1 failed | Hand-authored exact tests cover success, parse failure, no-match failure, and one outcome per command. No sway-captured command-reply fixtures exist, so untested commands and error paths have no conformance oracle. |
| `GET_VERSION` | Return `{}` | Caught: 23 passed, 1 failed | A wire test checks only `variant == "swayward"`. Changing `human_readable` to `BROKEN` passed all 24 IPC tests. No sway fixture or full key/type/value comparison exists. |
| `GET_BINDING_MODES` | Return `{}` instead of the configured mode-name array | Caught: focused IPC test failed | The live wire test checks the exact `default` plus configured mode list. |
| `GET_CONFIG` | Return `{}` instead of the single-field config reply | Caught: focused IPC test failed | The live wire test checks the exact top-level key and raw KDL value before and after reload. |
| `GET_INPUTS` | Omit or alter a captured value | Caught: focused socket test failed | The live wire test compares complete backend-neutral keyboard and pointer objects with a sway 1.11 fixture, including identifiers, names, types, pointer `scroll_factor`, repeat values, and XKB layout values. |
| `GET_INPUTS` libinput values | Omit USB IDs or alter a supported libinput property | Caught: focused socket test failed | A second sway 1.11 fixture captures a physical Logitech G703; the socket test compares its complete pointer object, including vendor, product, scroll factor, and every exposed libinput value. |
| `GET_SEATS` | Return focus ID 0 with a focused window | Caught: 146 passed, 1 failed | The live wire test checks the exact one-seat array, capability bitmask, focused container ID, and nested device array. |
| Workspace events | Change `change` from `reload` to `BROKEN` | Caught: focused event test failed | The headless subscriber reads a real event and compares its keys, JSON types, and `change` value with sway 1.11's `workspace.reload.json`. Fixtures also preserve every documented workspace change for future semantic tests. |
| Window events | Replace the payload with `{"broken":true}` | Caught: focused event test failed | The headless subscriber reads a real event and compares its keys, JSON types, and `change` value with sway 1.11's `window.focus.json`. Fixtures also preserve every documented window change for future semantic tests. |
| Mode events | Replace the payload with `{"broken":true}` | Caught: focused event test failed | The headless subscriber switches to a configured mode and compares the emitted event's keys, JSON types, and `change` value with sway 1.11's `mode.resize.json`. |
| Tick initial event | Omit the initial event | Caught: focused tick test failed | The test preserves coalesced frames while checking that the subscription reply arrives first and the initial payload is exactly `{"first":true,"payload":""}`. It also checks that a later `SEND_TICK` event has `first:false` and the requested payload. |
| Tick initial flag | Set the initial event's `first` field to `false` | Caught: focused tick test failed | The exact initial-payload comparison rejected the wrong flag. |
| Tick subscription scope | Send the initial tick to a workspace-only subscriber | Caught: focused non-tick test failed | The next frame had tick's event type instead of the injected workspace event, so the test rejected the unsolicited tick. |
| Output events | Omit output serialization or output-change emission | Caught: focused output test failed | Replacing the headless outputs through the production path must emit exactly `{"change":"unspecified"}` after the subscription reply. The test preserves a coalesced second frame. |
| Output subscription scope | Send the output event to a tick-only subscriber | Caught: focused non-output test failed | The next frame had output's event type instead of the injected tick barrier. |
| Shutdown events | Omit shutdown serialization | Caught: focused shutdown test failed | `State::request_stop("exit")` must emit exactly `{"change":"exit"}` after the subscription reply. |
| Shutdown subscription scope | Send the shutdown event to a workspace-only subscriber | Caught: focused non-shutdown test failed | The workspace-only subscriber received a byte instead of remaining empty. |
| Input hotplug events | Omit input serialization or hotplug emission | Caught: focused input test failed | Real device add and remove paths emit `added` and `removed` with the affected object exactly equal to the shared `GET_INPUTS` payload. |
| Input XKB events | Omit keymap or layout emission | Caught: focused input test failed | Keymap refresh emits `xkb_keymap`; switching to layout index 1 emits `xkb_layout`, and both objects equal the current `GET_INPUTS` keyboard payload. |
| Input subscription scope | Send the input event to a tick-only subscriber | Caught: focused non-input test failed | The next frame had input's event type instead of the injected tick barrier. |
| Event queue backpressure | Make the per-subscriber event channel unbounded | Caught: focused overflow tests failed | Non-reading output and input subscribers are disconnected when 4,097 queued events exceed the 4,096-event bound. The existing byte-buffer test separately checks disconnect after the 4 MB encoded-write limit. |
| Rectangle roles | Set a window's `geometry` equal to its outer `rect` | Caught: focused test failed | `live_ipc_rectangle_roles_match_sway_relationships` compares the equality relationship between each leaf's outer and geometry rects with sway's nested-tree fixture. |
| Rectangle roles | Set a window's `window_rect` equal to its outer `rect` | Caught: focused test failed | The focused test also requires the content box to remain contained within the outer box and to be strictly smaller in each dimension where sway's decorated fixture is smaller. |
| Rectangle roles | Set a window's `deco_rect` equal to its outer `rect` | Caught: focused test failed | The focused test compares the equality relationship between each leaf's outer and decoration rects and requires decorated leaves to report a non-empty titlebar. |
| Scratchpad branch | Remove the `__i3` output and `__i3_scratch` workspace | Caught: 18 passed, 6 failed | Both names and scratchpad presence are asserted. Scratchpad contents and focus/percent tests also depend on this branch. |

## What the fixture oracle does not mean

The 14 fixture scenarios cover `GET_TREE`, `GET_WORKSPACES`, and `GET_OUTPUTS`. They do not cover every IPC message. Most scalar fixture values are compared only by JSON type. Explicit value checks currently cover node `type`, focus arrays, percent values, `floating`, `scratchpad_state`, workspace `representation`, selected workspace/output identity fields, marks, scratchpad names, output positions, and root size.

The tests compare parsed JSON, not reply bytes. They do not check object-key order, whitespace, or numeric spelling. “Matches sway schema” therefore means the tested JSON structure and selected semantics, not byte-for-byte payload identity.

Arrays below the top-level workspace/output replies remain length-checked only for `nodes` and `floating_nodes`. Other nested arrays use the first fixture element as a shape template and can accept missing elements. Focus arrays have a separate exact semantic check.

Unsupported message types are not enumerated. The suite does not prove that every `MessageType` returns either a sway-compatible payload or a well-formed error.

## Event fixture boundary

`tests/fixtures/sway/events/` contains every documented workspace and window `change` value requested for the audit, plus `resize` and `default` mode payloads. The headless test exercises one deterministic event from each family. The remaining fixtures preserve real sway schemas but do not yet have one test per change value or semantic value checks beyond `change`.
