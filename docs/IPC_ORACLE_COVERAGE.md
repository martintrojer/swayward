This reference records what `src/tests/ipc.rs` verifies against sway and where it can still accept an incompatible reply. Treat each entry as narrow: a caught mutation proves only the listed behavior.

## Empirical method

For each row, one emitter behavior was changed, `cargo test -p swayward --lib tests::ipc` was run, and the change was reverted. A nonzero result means at least one IPC test caught the mutation. An exit code of 0 means every IPC test accepted it. The whole-value fixture test also mutates every scalar that survives normalization and requires each mutation to change the comparison result.

## Coverage matrix

| Surface | Mutation | Result | Boundary |
|---|---|---:|---|
| `GET_TREE` node schema and values | Change a stable scalar value, node key set, JSON value type, leaf name/layout/orientation, focus/percent, or rectangle beyond 10 px | Caught | The live test normalizes only run- or backend-dependent fields, then compares the whole parsed tree with a sway 1.12 capture. Dynamic IDs are mapped separately by tree position. Rectangles retain a 10 px tolerance and role checks because font metrics differ. The synthetic scratch workspace's `fullscreen_mode` is normalized because sway hard-codes 1 while swayward has no corresponding workspace-fullscreen state. |
| `GET_TREE` focus | Change focus MRU contents or order | Caught | `live_ipc_focus_matches_sway_mru_arrays` maps dynamic IDs and compares each node's focus array. |
| `GET_TREE` percent | Change parent shares, nullability, or a sibling sum | Caught | `live_ipc_percent_matches_sway_parent_shares` compares fixture values and sums. |
| `GET_WORKSPACES` | Change a stable scalar value or array length | Caught | The live test normalizes dynamic IDs, the backend output name, and rectangles, then compares the complete parsed reply with sway 1.12. |
| `GET_OUTPUTS` | Change a stable scalar value, omit `hdr`, or change `features` | Caught | The live test normalizes backend identity, advertised modes, refresh, and adaptive-sync capability, then compares the complete parsed reply with sway 1.12. Configured `hdr`, `features.hdr`, state flags, scale, transform, subpixel layout, and other stable values remain exact. A separate rotated-output test verifies sway's clockwise transform spelling. The reply remains Partial because backend adaptive-sync, tearing, HDR, render-time, and power state do not reach this serializer. |
| `GET_MARKS` | Return `[]` after creating a mark | Caught: 17 passed, 2 failed | Tests compare the complete one-mark reply and an empty reply after unmark. There is no sway-captured `GET_MARKS` fixture. |
| `RUN_COMMAND` | Replace every failure outcome with `[{"success":true}]` | Caught: 23 passed, 1 failed | Hand-authored exact tests cover success, parse failure, no-match failure, and one outcome per command. No sway-captured command-reply fixtures exist, so untested commands and error paths have no conformance oracle. |
| `GET_VERSION` | Return `{}` | Caught: 23 passed, 1 failed | A wire test checks only `variant == "swayward"`. Changing `human_readable` to `BROKEN` passed all 24 IPC tests. No sway fixture or full key/type/value comparison exists. |
| `GET_BINDING_MODES` | Return `{}` instead of the configured mode-name array | Caught: focused IPC test failed | The live wire test checks the exact `default` plus configured mode list. |
| `GET_CONFIG` | Return sway's single-field config reply containing KDL | Caught: focused IPC test failed | The live wire test asserts `{"success": false}` and that no `config` key is present, so re-serving KDL in sway's envelope fails. |
| `GET_INPUTS` | Omit or alter a captured value | Caught: focused socket test failed | The live wire test compares complete backend-neutral keyboard and pointer objects with a sway 1.12 fixture, including identifiers, names, types, pointer `scroll_factor`, repeat values, and XKB layout values. |
| `GET_INPUTS` libinput values | Omit USB IDs or alter a supported libinput property | Caught: focused socket test failed | A second sway 1.11 fixture captures a physical Logitech G703; the socket test compares its complete pointer object, including vendor, product, scroll factor, and every exposed libinput value. |
| `GET_SEATS` | Return focus ID 0 with a focused window | Caught: 146 passed, 1 failed | The live wire test checks the exact one-seat array, capability bitmask, focused container ID, and nested device array. |
| Workspace events | Change `change` from `reload` to `BROKEN` | Caught: focused event test failed | The headless subscriber reads a real event and compares its keys, JSON types, and `change` value with sway 1.11's `workspace.reload.json`. Sway 1.12 sequence captures additionally pin order and multiplicity for workspace switching, closing the last window, renaming, and cross-output moves. |
| Window events | Replace the payload with `{"broken":true}` | Caught: focused event test failed | The headless subscriber reads a real event and compares its keys, JSON types, and `change` value with a sway fixture. The sway 1.12 window-map sequences additionally pin the `tag` field. Individual event fixtures remain 1.11 captures and most are not emitted one by one in tests. |
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

<!-- compatibility-census:oracle-fixtures:start -->
The 14 fixture scenarios cover `GET_TREE`, `GET_WORKSPACES`, and `GET_OUTPUTS`.
<!-- compatibility-census:oracle-fixtures:end -->
They do not cover every IPC message. For the one-window scenario, stable values
in `GET_TREE`, `GET_WORKSPACES`, and `GET_OUTPUTS` are compared as complete
parsed JSON subtrees. The normalization list is centralized in
`normalize_fixture_value`. It covers dynamic node and process IDs; output names,
hardware identity, modes, refresh, and adaptive-sync capability; focus arrays
whose IDs are compared separately by position; font-dependent rectangles; and
the synthetic scratch workspace's deliberately different fullscreen state.
The representative one-window fixture retains 110 tree, 17 workspace, and 29
output scalar values after normalization. It also retains 18 tree, 4 workspace,
and 4 output arrays. A mutation test changes every retained scalar and array
length in turn and requires the comparison to fail. Other
fixture scenarios still provide complete key/type coverage and focused semantic
checks for nested layout, focus, percentages, floating state, marks, and
stacking.

Every nested array is length- and order-checked except output `modes`. Mode
enumeration is backend-dependent: the nested Wayland capture advertises no
modes, while the headless Smithay test output advertises its synthetic current
mode. Focus arrays have a separate exact semantic check after dynamic IDs are
mapped by tree position.

The tests compare parsed JSON, not reply bytes. They do not check object-key
order, whitespace, or numeric spelling. “Matches sway schema” therefore means
the tested JSON structure and selected semantics, not byte-for-byte payload
identity.

`contrib/check-sway-fixture-schema /path/to/sway` requires the checkout to be the
commit pinned in `tests/fixtures/sway/schema-version.json`. It extracts the
`GET_OUTPUTS`, output-node, native-view, and output-feature field sets from
sway's serializers and compares them with representative fixtures. CI clones
the pinned tag and runs this check, so a target-version bump cannot silently
reuse stale fixture schemas.

Unsupported message types are now enumerated. `every_message_type_replies_and_leaves_the_connection_usable` walks 0..=13 plus 99, 100, 101, 102, 1000 and `u32::MAX`, and requires each to echo its request type, return parseable JSON, and be an object or array; anything outside sway's supported set must report `success: false`, and type 11 must equal sway's `IPC_SYNC` decline exactly. The connection must still serve a real request afterwards.

`malformed_frames_do_not_hang_or_wedge_the_server` covers a truncated header, bad magic, a length field longer than the payload, and a non-UTF-8 payload. The server may reply or drop that client, but it must keep serving a second one, which is the testable half of "it never hangs".

## Event fixture boundary

`tests/fixtures/sway/events/` contains every documented workspace and window `change` value requested for the audit, plus `resize` and `default` mode payloads. The headless test exercises one deterministic event from each family. The remaining fixtures preserve real sway schemas but do not yet have one test per change value or semantic value checks beyond `change`.
