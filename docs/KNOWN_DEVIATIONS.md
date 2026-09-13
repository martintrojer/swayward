# Known deviations from sway

This reference lists user-visible differences from sway. It does not list
internal changes inherited from the niri fork. See `docs/DIVERGENCE.md` for that
engineering ledger.

## Configuration

Swayward does not parse sway configuration files. It uses typed KDL configuration
to retain niri's nested effect and animation settings. Use the
[sway config migration guide](SWAY_CONFIG_MIGRATION.md) to translate an existing
configuration.

Bindings can contain sway command strings:

```kdl
binds {
    Mod+H { command "focus left"; }
}
```

Swayward also retains typed niri actions for features outside the current sway
command subset.

## Bars

Swayward has no `bar {}` configuration block and does not launch swaybar.
`GET_BAR_CONFIG` returns an empty array.

Use waybar as an external layer-shell client. Waybar 0.15.0 was tested manually
with its `sway/workspaces`, `sway/window`, and `sway/mode` modules. The test used
a nested compositor and is not part of the automated suite.

## IPC requests and commands

`GET_CONFIG`, `GET_INPUTS`, and `GET_SEATS` return:

```json
{"success":false,"error":"not implemented"}
```

`SEND_TICK` and `GET_BINDING_STATE` currently return the same error. Commands
outside the implemented subset return a sway-shaped `RUN_COMMAND` failure. See
the [compatibility matrix](SWAY_COMPATIBILITY.md) for the supported subset and
the [IPC oracle coverage](IPC_ORACLE_COVERAGE.md) for its test boundary.

## Layout and Xwayland

Sway rejects `layout toggle stacked` because its two-token form accepts only
`split` or `all` (`sway/commands/layout.c:57-71`). i3 accepts the command as a
no-op. Swayward follows sway and reports a command error.

Swayward has no scrollable-tiling mode. It uses an i3-style nested container tree.
Niri's horizontal viewport and overview animations were retired because their
layout no longer exists.

### The i3 `open` command and empty containers

i3's `open` command creates and focuses an empty container
(`i3/src/commands.c:1726-1740`). Sway does not implement this command. It is
absent from sway's complete general, configuration-only, and runtime-only
command tables (`sway/sway/commands.c:44-144`) and from the runtime command
reference (`sway/sway/sway.5.scd:102-415`).

Swayward follows sway. The command parser returns a well-formed failure for
`open`, as required by the IPC compatibility decisions Q1, Q8, and Q11. It does
not create i3 empty containers.

The i3 test adapter cannot preserve this behavior. Its `cmd 'open'` and
`open_empty_con` paths create a real Wayland window through the test control
socket (`tests/i3/lib/i3test.pm:97-114`). Assertions that pass after this
substitution test ordinary window behavior, not empty-container behavior.
Tests whose result depends on a real i3 empty container remain permanently
excluded from the passing manifest.

### Floating container wrappers

The i3 tree wraps each floating window in a `floating_con`. The wrapper is in
the workspace's `floating_nodes`, and the window is in the wrapper's `nodes`
(`i3/src/floating.c:280-351` and `i3/src/ipc.c:619-633`).

Sway stores each floating container directly in the workspace's floating list
and serializes that container into `floating_nodes`
(`sway/sway/ipc-json.c:532-540`). A floating leaf therefore has no child in its
`nodes` array because the recursive serializer only emits actual container
children (`sway/sway/ipc-json.c:854-893`). Sway also appends a newly floating
container to the end of this list (`sway/sway/tree/workspace.c:961-971`), while
i3 inserts a new floating wrapper at the front (`i3/src/floating.c:280-295`).
Swayward follows sway's hierarchy and list order.

The final assertion in i3's `141-resize.t` searches only the children of each
floating node. It cannot find either direct floating leaf, so it does not test
the targeted resize result against sway or swayward. The preceding assertion
confirms that the untargeted floating window is unchanged. This assertion is
excluded as an i3-only tree-shape check.

The same difference affects 13 assertions in `135-floating-focus.t`: assertions
31, 32, 34, 35, 37, 40, 43, 46, 54, 62, 66, 73, and 74. Some traverse the i3
floating wrapper. Others compare i3's X11 `window` field, which sway emits only
for Xwayland views (`sway/sway/ipc-json.c:667-693`); the adapter creates Wayland
clients, whose identity is the container `id`. Replacing only those lookups with
the direct sway node `id` makes all 13 assertions pass. This check preserves the
underlying assertions about floating stack order, layer membership, and nested
position without fabricating nodes in swayward's IPC tree.

### Output content nodes

The i3 `GET_TREE` hierarchy places a container named `content` between each
output and its workspaces. The i3 IPC guide shows this hierarchy at
`i3/docs/ipc:485-497`, and i3 creates the node in `i3/src/tree.c:38-55`.

Sway has no equivalent node. Its node types are root, output, workspace, and
container (`sway/include/sway/tree/node.h:18-23`). Its `GET_TREE` serializer
adds workspaces directly to output nodes (`sway/sway/ipc-json.c:854-894`). None
of the 14 trees captured from real sway in `tests/fixtures/sway/*.tree.json`
contains a `content` node. Swayward therefore follows sway, as required by the
IPC compatibility decisions Q1 and Q8.

The i3 conformance adapter does not synthesize this node. Fabricating a node in
the adapter would make an upstream assertion observe a tree that a real sway IPC
client never receives. Tests that directly traverse or inspect i3's `content`
node are excluded as i3-only tree-structure tests.

### Marks applied to several matching containers

The i3 command `[criteria] mark name` fails when the criteria match more than
one container. Sway runs the command once for every match
(`sway/sway/commands.c:301-326`). Each run removes the mark from its previous
container before adding it to the current one
(`sway/sway/commands/mark.c:46-58` and
`sway/sway/tree/container.c:1639-1654`). The command therefore succeeds, and the
last matched container retains the mark.

Swayward follows sway. Assertions 14, 15, and 17 in i3's
`210-mark-unmark.t` expect i3's rejection and are excluded.

X11 applications run through `xwayland-satellite`. Swayward does not include
sway's in-process Xwayland window manager.
