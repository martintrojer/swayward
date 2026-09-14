# IPC oracle coverage

This reference records what `src/tests/ipc.rs` verifies against sway and where it can still accept an incompatible reply. Treat each entry as narrow: a caught mutation proves only the listed behavior.

## Empirical method

For each row, one emitter behavior was changed, `cargo test -p swayward --lib tests::ipc` was run, and the change was reverted. A nonzero result means at least one IPC test caught the mutation. An exit code of 0 means all 24 IPC tests accepted it.

The array mutations below were run during the M3 oracle fix. The remaining mutations were run during this audit.

## Coverage matrix

| Surface | Mutation | Result | Boundary |
|---|---|---:|---|
| `GET_TREE` node schema | Change a node key set or a JSON value type | Caught | `assert_same_shape` and `assert_node_schema_appears_in_fixtures` check recursive key sets, JSON types, and node `type` values against 14 sway fixtures. |
| `GET_TREE` focus | Change focus MRU contents or order | Caught | `live_ipc_focus_matches_sway_mru_arrays` maps dynamic IDs and compares each node's focus array. |
| `GET_TREE` percent | Change parent shares, nullability, or a sibling sum | Caught | `live_ipc_percent_matches_sway_parent_shares` compares fixture values and sums. |
| `GET_WORKSPACES` | Return `[]` | Caught: 18 passed, 1 failed | The live wire test checks one entry and its `num`, `name`, and `output`. Fixture comparison also checks top-level length and element shape. Other scalar values remain shape-only unless a focused test covers them. |
| `GET_OUTPUTS` | Return `[]` | Caught | The live wire test checks one entry and its `name`. Fixture comparison also checks top-level length and element shape. Other scalar values remain shape-only unless a focused test covers them. |
| `GET_MARKS` | Return `[]` after creating a mark | Caught: 17 passed, 2 failed | Tests compare the complete one-mark reply and an empty reply after unmark. There is no sway-captured `GET_MARKS` fixture. |
| `RUN_COMMAND` | Replace every failure outcome with `[{"success":true}]` | Caught: 23 passed, 1 failed | Hand-authored exact tests cover success, parse failure, no-match failure, and one outcome per command. No sway-captured command-reply fixtures exist, so untested commands and error paths have no conformance oracle. |
| `GET_VERSION` | Return `{}` | Caught: 23 passed, 1 failed | A wire test checks only `variant == "swayward"`. Changing `human_readable` to `BROKEN` passed all 24 IPC tests. No sway fixture or full key/type/value comparison exists. |
| `GET_BINDING_MODES` | Return `{}` instead of the configured mode-name array | Caught: focused IPC test failed | The live wire test checks the exact `default` plus configured mode list. |
| `GET_CONFIG` | Return `{}` instead of the normal not-implemented error | Not caught: 24 passed | No request or schema assertion exists. |
| `GET_INPUTS` | Return `{}` instead of the normal not-implemented error | Not caught: 24 passed | No request or schema assertion exists. |
| `GET_SEATS` | Return `{}` instead of the normal not-implemented error | Not caught: 24 passed | No request or schema assertion exists. |
| Workspace events | Change `change` from `reload` to `BROKEN` | Caught: focused event test failed | The headless subscriber reads a real event and compares its keys, JSON types, and `change` value with sway 1.11's `workspace.reload.json`. Fixtures also preserve every documented workspace change for future semantic tests. |
| Window events | Replace the payload with `{"broken":true}` | Caught: focused event test failed | The headless subscriber reads a real event and compares its keys, JSON types, and `change` value with sway 1.11's `window.focus.json`. Fixtures also preserve every documented window change for future semantic tests. |
| Mode events | Replace the payload with `{"broken":true}` | Caught: focused event test failed | The headless subscriber switches to a configured mode and compares the emitted event's keys, JSON types, and `change` value with sway 1.11's `mode.resize.json`. |
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
