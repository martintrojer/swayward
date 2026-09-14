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

### Focus wrapping modes

Sway accepts `focus_wrapping yes|no|force|workspace`; its parser also treats the seven true boolean words as `yes`, compares `force` and `workspace` case-insensitively, and treats every other value as `no` (`sway/commands/focus_wrapping.c:6-22`; `common/util.c:40-52`). Swayward currently represents only `yes` and `force`. The config translator maps those exact modes, including the seven true words, and reports `no`, `workspace`, `toggle`, and other false-valued forms for manual conversion rather than changing their behavior. Both compositors default to `yes` (`sway/config.c:274`; `swayward-config/src/layout.rs`).

### Pointer focus and warping defaults

Sway enables `focus_follows_mouse` by default (`sway/sway/config.c:272`).
Swayward retains niri's opt-in setting, so the shipped configuration leaves
`focus-follows-mouse` commented out (`resources/default-config.kdl:68`). Enable
that setting to use sway's default pointer-focus behavior.

Sway defaults `mouse_warping` to `output` (`sway/sway/config.c:273`): it warps
only when focus crosses an output boundary (`sway/sway/input/seat.c:1530-1548`).
Swayward leaves `warp-mouse-to-focus` disabled by default
(`resources/default-config.kdl:64-65`). The config translator maps
`mouse_warping none` to that disabled default and `mouse_warping container` to
`warp-mouse-to-focus mode="center-xy"`. It reports `mouse_warping output` for
manual conversion because swayward cannot express output-only warping without
also enabling within-output window warps.

### Edge-border hiding

Sway has two independent edge-decoration settings. `hide_edge_borders` has six
case-sensitive values: `none`, `vertical`, `horizontal`, `both`, `smart`, and
`smart_no_gaps`; the optional `--i3` flag separately enables `hide_lone_tab`
(`sway/commands/hide_edge_borders.c:7-45`). The directional modes suppress
individual tiled-window edges at workspace boundaries. `smart` suppresses every
edge only when the tiled view is the sole visible view, while `smart_no_gaps`
does so only when the workspace's current outer gaps are also all zero. Floating
windows are excluded (`sway/tree/view.c:309-409`). `hide_lone_tab` separately
removes a singleton tabbed or stacked titlebar for non-normal border styles
(`sway/desktop/transaction.c:316-347`; `sway/sway/ipc-json.c:543-555`).

Swayward's border model has one symmetric width used by geometry and rendering,
and its titlebar model has no singleton suppression setting. The translator
therefore accepts only `hide_edge_borders none`, which is already swayward's
behavior, and reports every other mode and every `--i3` form for manual
conversion. It does not map them to `border off`, because that would incorrectly
remove floating and multi-window borders. Swayward's shipped 4 px border and
16 px gaps remain unchanged.

## Bars

Swayward has no `bar {}` configuration block and does not launch swaybar.
`GET_BAR_CONFIG` returns an empty array.

Use waybar as an external layer-shell client. Waybar 0.15.0 was tested manually
with its `sway/workspaces`, `sway/window`, and `sway/mode` modules. The test used
a nested compositor and is not part of the automated suite.

## IPC requests and commands

### Version identity

`GET_VERSION` uses sway's six-field reply schema: `human_readable`, `variant`,
`major`, `minor`, `patch`, and `loaded_config_file_name`. Sway defines that
schema in `sway/sway/ipc-json.c:225-238`; the fixture capture records sway 1.11
as `variant: sway`, version `1.11.0` (`tests/fixtures/sway/README.md:9`).
Swayward reports its own variant and package version in those fields rather than
claiming to be sway or i3. Therefore, i3's `193-ipc-version.t` assertion that
the major version is always 4 does not apply.

`GET_CONFIG`, `GET_INPUTS`, and `GET_SEATS` return:

```json
{"success":false,"error":"not implemented"}
```

`GET_BINDING_STATE` currently returns the same error. Commands
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

### Layout restoration

Sway does not implement i3's `append_layout` command or JSON placeholder
containers. The command is absent from sway's complete general,
configuration-only, and runtime-only command tables
(`sway/sway/commands.c:44-144`) and from its runtime command reference
(`sway/sway/sway.5.scd:102-415`). The similarly named `client.placeholder`
entry only accepts an obsolete color setting as a no-op
(`sway/sway/commands.c:55`).

Swayward follows sway and rejects `append_layout`. The i3 layout-restore family
(`213`–`216`) is therefore skipped rather than gaining an engine that sway does
not expose.

### The i3 `open` command and empty containers

i3's `open` command creates and focuses an empty container
(`i3/src/commands.c:1726-1740`). Sway does not implement this command. It is
absent from sway's complete general, configuration-only, and runtime-only
command tables (`sway/sway/commands.c:44-144`) and from the runtime command
reference (`sway/sway/sway.5.scd:102-415`).

Swayward follows sway. The command parser returns a well-formed failure for
`open`, as required by the IPC compatibility decisions Q1, Q8, and Q11. It does
not create i3 empty containers.

### Directional output moves do not wrap

I3 wraps `move container to output <direction>` from the edge output to the
opposite edge. Sway resolves the destination with
`wlr_output_layout_adjacent_output` and returns no destination when no output is
adjacent (`sway/sway/tree/output.c:316-330`; `sway/sway/commands/move.c:277-309`).
Swayward follows sway, so the two wrap assertions in `512-move-wraps.t` are
skipped. This behavior is independent of the `focus_wrapping` configuration.
A sway 1.11 run with two horizontal headless outputs confirmed the source: the
first `move container to output right` moved the view from x=0 to x=800. The
second command failed with `Can't move an empty workspace`, and the view stayed
at x=800.

### Directional floating moves in percentage points

The i3 command `move right 25 ppt` moves a floating container by 25 percent of
the output width. Sway's directional move parser reads only the numeric first
argument and ignores the trailing `ppt` token (`sway/commands/move.c:672-681`).
It therefore moves the container by 25 pixels (`sway/commands/move.c:693-710`).
Swayward follows sway's pixel-only directional movement rather than i3's
percentage-point behavior.

### Workspace names beginning with `__`

I3 reserves workspace names beginning with `__`: it excludes such names while
collecting startup workspace bindings and rejects them in workspace switch,
container move, and rename commands (`i3/src/workspace.c:231`;
`i3/src/commands.c:318,912,2116`).

Sway does not reserve that prefix. Its workspace command creates an arbitrary
name when no workspace matches (`sway/sway/commands/workspace.c:223-227`), its
move command likewise creates an arbitrary destination
(`sway/sway/commands/move.c:450-504`), and rename rejects special command words
rather than an `__` prefix (`sway/sway/commands/rename.c:72-82`). Startup
workspace discovery also accepts arbitrary binding targets after excluding only
workspace command words (`sway/sway/tree/workspace.c:356-490`). Sway's
`__i3` output and `__i3_scratch` workspace are synthetic GET_TREE nodes, not
reserved user-workspace identities (`sway/sway/ipc-json.c:459-499`).

Swayward follows sway and permits names such as `__foo`. The i3 adapter excludes
the synthetic `__i3` output when implementing `get_workspace_names`, but does
not hide real user-created `__*` workspaces. Assertions requiring i3's prefix
restriction are skipped rather than adding a workspace-name guard that sway
does not have.

### Workspace rename edge cases

Sway parses `rename workspace to to bla` as the current-workspace form and uses
all arguments after the first `to` as the new name, producing `to bla`
(`sway/sway/commands/rename.c:36-38,66-72`). i3's `117-workspace.t` expects the
same command to rename workspace `to` to `bla`; swayward follows sway, so that
assertion is skipped.

Sway workspace lookup is case-insensitive (`sway/sway/tree/workspace.c:508-513`).
When the requested new name differs only by case, the rename command finds the
same workspace and returns success without changing its spelling
(`sway/sway/commands/rename.c:84-92`). i3 expects `11: bar` to become `11: BAR`;
swayward follows sway's no-op behavior, so the spelling assertion is skipped.

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

Several i3 conformance assertions cannot observe the equivalent sway behavior
because they traverse the wrapper's child instead of the direct floating node:

- The final assertion in `141-resize.t` cannot find either floating leaf. The
  preceding assertion confirms that the untargeted floating window is unchanged.
- Assertions 40–43 in `156-fullscreen-focus.t` inspect the child count below an
  i3 fullscreen wrapper after workspace moves.
- All six assertions in `236-floating-focus-raise.t` inspect the wrapper child.
  Replacing only that lookup in a temporary diagnostic makes all six pass.
  Swayward therefore raises the focused float correctly and serializes the list
  back-to-front. The behavior is also pinned by the real sway captures
  `one_floating.tree.json`, `three_floating_before_raise.tree.json`, and
  `three_floating_after_raise.tree.json`: their order changes from `[1, 2, 3]` to
  `[2, 3, 1]` after raising the first window. The fixture test was
  mutation-verified when added.

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

### Urgency for assigned windows

When i3 assigns a new window to an invisible workspace, it marks the window
urgent (`i3/src/manage.c:288-316`). Sway selects the assigned workspace before
mapping and declines to focus a view whose target workspace is not active
(`sway/sway/tree/view.c:628-665,696-732`), but its map path does not call
`view_set_urgent` (`sway/sway/tree/view.c:930-969`).

Swayward follows sway. Assignment to an invisible workspace does not make the
window or workspace urgent. Assignment to a visible workspace, including one
visible on another output, also leaves urgency clear.

### Marks applied to several matching containers

The i3 command `[criteria] mark name` fails when the criteria match more than
one container. Sway runs the command once for every match
(`sway/sway/commands.c:301-326`). Each run removes the mark from its previous
container before adding it to the current one
(`sway/sway/commands/mark.c:46-58` and
`sway/sway/tree/container.c:1639-1654`). The command therefore succeeds, and the
last matched container retains the mark.

Swayward follows sway. The
`multi_target_mark_moves_to_last_match_and_unmark_clears_every_match` test pins
both effects: `mark` leaves the mark only on the last match, and a criteria-driven
`unmark` clears every matched container. The latter matches sway's per-match
command loop and its targeted clear operation
(`sway/sway/commands/unmark.c:24-54`). Assertions 14, 15, and 17 in i3's
`210-mark-unmark.t` expect i3's multi-target `mark` rejection and are excluded.

X11 applications run through `xwayland-satellite`. Swayward does not include
sway's in-process Xwayland window manager. The satellite presents X11 clients as
ordinary `xdg_toplevel` surfaces. The xdg-shell protocol exposes one `app_id`
and one title, but no separate X11 class, instance, or `WM_WINDOW_ROLE` values
(`xdg-shell.xml`, `xdg_toplevel.set_app_id`). Swayward therefore cannot evaluate
those X11-only criteria unless the satellite supplies a metadata protocol and
swayward stores and exposes the metadata. Sway's in-process Xwayland path does
both (`sway/criteria.c:355-410`; `sway/sway/ipc-json.c:670-700`).

## Workspace ordering and implicit workspaces

Sway sorts each output's workspace list when a workspace is created
(`sway/sway/tree/workspace.c:255-259` calls `output_sort_workspaces` immediately
after `output_add_workspace`), and navigation observes that stored order.
Swayward inherits niri's trailing unnamed workspace as an internal creation
target and does not sort on creation, so `workspace next` and `workspace prev`
can visit workspaces in a different order than sway.

No placeholder workspace is ever reported to clients: `GET_WORKSPACES` and
`GET_TREE` show only workspaces you created. Workspace numbers and names match
sway, including `num = -1` for names without a leading digit
(`sway/sway/ipc-json.c:503-517`).

Aligning the order needs a workspace identity and lifecycle representation
distinct from niri's name and persistence state, because "sway-visible" differs
between IPC, relative navigation, and cleanup. The analysis, including the two
reverted attempts and the exact conformance cost, is recorded in
`tests/i3/README.md`.

## Floating split containers

Sway can float a whole split container. `cmd_floating` selects the focused
container, wraps every tiling child when the workspace itself is selected,
promotes a child of an existing floating root to that root, then calls
`container_set_floating`, which detaches the selected node as one unit
(`sway/sway/commands/floating.c:23-55`).

Swayward floats windows, not containers. `FloatingSpace` stores a
`Vec<Tile<W>>` of leaves rather than tree nodes
(`src/layout/floating.rs:36-38`), and `Workspace::toggle_window_floating`
accepts a single window id (`src/layout/workspace.rs:1641`). So
`floating enable` on a focused split floats nothing.

Supporting it needs a floating container representation plus coordinated
rendering, geometry, focus, IPC, toggle-back, workspace-move, scratchpad and
invariant work. That is a deliberate deferral, not an oversight: I5 makes the
upstream diff a budget and Q7 keeps `floating.rs` close to niri so upstream
merges stay viable. The conformance cost is recorded against
`155-floating-split-size.t`, `184-regress-float-split-resize.t`, and
`206-fullscreen-scratchpad.t` in `tests/i3/README.md`.
