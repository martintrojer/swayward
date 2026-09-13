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
| `GET_BINDING_MODES` | Return `{}` instead of the normal not-implemented error | Not caught: 24 passed | No request or schema assertion exists. |
| `GET_CONFIG` | Return `{}` instead of the normal not-implemented error | Not caught: 24 passed | No request or schema assertion exists. |
| `GET_INPUTS` | Return `{}` instead of the normal not-implemented error | Not caught: 24 passed | No request or schema assertion exists. |
| `GET_SEATS` | Return `{}` instead of the normal not-implemented error | Not caught: 24 passed | No request or schema assertion exists. |
| Workspace events | Change `change` from `reload` to `BROKEN` | Not caught: 24 passed | Subscription acknowledgement and concurrent queries are tested. No test reads a workspace event payload, and no event fixture exists. |
| Window events | Replace the payload with `{"broken":true}` | Not caught: 24 passed | No test reads a window event payload, and no event fixture exists. |
| Mode events | Add a malformed mode-event payload | Not caught: 24 passed | `mode` subscriptions are accepted, but the server does not currently emit sway mode events. No event fixture exists. |
| Rectangle roles | Set a window's `geometry` equal to its outer `rect` | Not caught: 24 passed | The oracle checks that `rect`, `window_rect`, `deco_rect`, and `geometry` have the expected object shape. It does not prove that they are four semantically distinct boxes. |
| Rectangle roles | Set a window's `window_rect` equal to its outer `rect` | Not caught: 24 passed | Same gap. |
| Rectangle roles | Set a window's `deco_rect` equal to its outer `rect` | Not caught: 24 passed | Same gap. Focused tests cover output positions and the root size only. |
| Scratchpad branch | Remove the `__i3` output and `__i3_scratch` workspace | Caught: 18 passed, 6 failed | Both names and scratchpad presence are asserted. Scratchpad contents and focus/percent tests also depend on this branch. |

## What the fixture oracle does not mean

The 14 fixture scenarios cover `GET_TREE`, `GET_WORKSPACES`, and `GET_OUTPUTS`. They do not cover every IPC message. Most scalar fixture values are compared only by JSON type. Explicit value checks currently cover node `type`, focus arrays, percent values, `floating`, `scratchpad_state`, workspace `representation`, selected workspace/output identity fields, marks, scratchpad names, output positions, and root size.

The tests compare parsed JSON, not reply bytes. They do not check object-key order, whitespace, or numeric spelling. “Matches sway schema” therefore means the tested JSON structure and selected semantics, not byte-for-byte payload identity.

Arrays below the top-level workspace/output replies remain length-checked only for `nodes` and `floating_nodes`. Other nested arrays use the first fixture element as a shape template and can accept missing elements. Focus arrays have a separate exact semantic check.

Unsupported message types are not enumerated. The suite does not prove that every `MessageType` returns either a sway-compatible payload or a well-formed error.

## Follow-up priority

Event fixtures are the highest-value missing oracle. Waybar consumes workspace, window, and mode events, but all three malformed-event mutations passed. Capture representative payloads from sway and add headless subscription tests that read and compare emitted events.
