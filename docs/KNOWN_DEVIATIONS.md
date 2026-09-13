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

Swayward has no scrollable-tiling mode. It uses an i3-style nested container tree.
Niri's horizontal viewport and overview animations were retired because their
layout no longer exists.

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

X11 applications run through `xwayland-satellite`. Swayward does not include
sway's in-process Xwayland window manager.
