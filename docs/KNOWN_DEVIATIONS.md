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

### Bound command validation

Swayward validates every `command "..."` bind while it loads the KDL file. An
invalid command makes the config load fail before the compositor applies that
config (`swayward-config/src/binds.rs`; `swayward-ipc/src/command.rs`). This is
a deliberate safety difference from sway: sway parses and stores a binding's
command text, then validates the command only when the binding runs. For
example, sway accepts `bindsym X resize` into its configuration and reports the
missing resize arguments when `X` is pressed (`sway/sway/commands.c:131`;
`sway/sway/commands/bind.c:389-463`; `sway/sway/commands/resize.c:558-570`).

This difference can reject a sway configuration during migration even when the
invalid binding is never used. Swayward keeps eager validation because a typed
configuration should report broken actions at load time instead of preserving
a latent runtime error. The sway-to-KDL translator reports incomplete or
invalid bindings for manual correction; it does not omit them and apply a
partial configuration.

An empty binding such as `bindsym X` is invalid in both compositors. Sway rejects
it because `bindsym` requires both a key and a command
(`sway/sway/commands/bind.c:329-396`). The translator also rejects it as a
malformed directive.

### Include command substitution

Sway expands each `include` argument with `wordexp(3)`. This supports tilde,
variables, globs, and shell command substitution. Sway first changes to the
parent config's directory, then restores the working directory
(`sway/sway/config.c:594-625`). Command substitution means that loading a sway
config can execute arbitrary commands.

The `sway-to-kdl` translator is deliberately safer. It expands tilde, sway
variables, and globs relative to the file containing the directive, and it
recursively translates the resulting files. It refuses both backtick and
`$(...)` command substitution without executing them. Translation is commonly
run on a config before the user has reviewed it, so reproducing `wordexp` in
full would turn a static migration tool into an arbitrary-code execution path.

Sway also resolves each included name with `realpath` and keeps the canonical
path in `config_chain`; a path already in that list is not loaded again
(`sway/sway/config.c:555-591`). The translator likewise canonicalizes visited
paths. Duplicate includes and cycles therefore terminate without translating a
file twice.

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

Swayward represents the directional mode as `hide-edge-borders` and the smart
mode as `smart-borders`, so later `smart_borders` directives can override the
smart flag without changing the edge mode. The translator accepts all six
`hide_edge_borders` values and sway's `smart_borders` values. It keeps every
`--i3` form fail-loud because the titlebar model has no singleton suppression
setting. Swayward's shipped 4 px border and 16 px gaps remain unchanged.

## Bars

Swayward does not implement sway's `bar {}` configuration or launch swaybar.
Sway stores each bar block, launches the configured bar process, and exposes its
ID and complete settings through `GET_BAR_CONFIG` (`sway/sway/commands/bar.c`;
`sway/sway/ipc-server.c:846-878`; `sway/sway/ipc-json.c:1271-1466`). Implementing
that configuration surface without shipping or managing a bar would preserve
settings that no swayward component uses.

An empty `GET_BAR_CONFIG` payload returns `[]`, because swayward has no configured
bars. A non-empty payload names a bar ID. Because no ID can exist, swayward
returns `{"success":false,"error":"No bar with that ID"}`, matching sway's
missing-ID response instead of returning the list-shaped empty result.

Configure Waybar directly as an external layer-shell client. Waybar 0.15.0 was
tested manually with its `sway/workspaces`, `sway/window`, and `sway/mode`
modules. The test used a nested compositor and is not part of the automated
suite. Users lose swaybar configuration through compositor config and IPC,
including bar outputs, fonts, colors, status commands, and tray settings.

## IPC requests and commands

### Version identity

`GET_VERSION` uses sway's six-field reply schema: `human_readable`, `variant`,
`major`, `minor`, `patch`, and `loaded_config_file_name`. Sway defines that
schema in `sway/sway/ipc-json.c:225-238`; the fixture capture records sway 1.11
as `variant: sway`, version `1.11.0` (`tests/fixtures/sway/README.md:9`).
Swayward reports its own variant and package version in those fields rather than
claiming to be sway or i3. Therefore, i3's `193-ipc-version.t` assertion that
the major version is always 4 does not apply.

`GET_INPUTS` omits sway's pointer `scroll_factor` and libinput-only `libinput`,
`vendor`, and `product` fields. Swayward applies separate horizontal and vertical
scroll factors after input dispatch and has no single value equivalent to sway's
scalar setting. Smithay's backend-neutral device trait does not expose libinput
configuration or IDs; the compatible `identifier`, `name`, `type`, keyboard
repeat, and XKB layout fields are returned instead (`sway/sway/ipc-json.c:1155-1228`).

Commands outside the implemented subset return a sway-shaped `RUN_COMMAND`
failure. See
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

### Workspace output lists

I3 accepts `move workspace to output next` and lists of output names. It cycles
each matched workspace through the configured list. Sway accepts only one output
name, id, or geometric direction for this command. Its workspace mover resolves
`argv[0]` and ignores later arguments (`sway/sway/commands/move.c:30-78,630-669`).
Consequently, `next` is an unknown output and a list always selects its first
valid name.

Swayward follows sway's single-target grammar. The output-cycle assertions in
`543-move-workspace-to-multiple-outputs.t` are skipped rather than adding i3-only
selection semantics.

### Directional floating moves in percentage points

The i3 command `move right 25 ppt` moves a floating container by 25 percent of
the output width. Sway's directional move parser reads only the numeric first
argument and ignores the trailing `ppt` token (`sway/commands/move.c:672-681`).
It therefore moves the container by 25 pixels (`sway/commands/move.c:693-710`).
Swayward follows sway's pixel-only directional movement rather than i3's
percentage-point behavior.

### Singleton layout containers after moves

Sway preserves singleton stacked and tabbed containers after moving their other
children away. Move paths call `container_reap_empty`, which destroys only
containers with zero children (`sway/sway/tree/container.c:525-541`;
`sway/sway/commands/move.c:225,410,612,722`). The separate
`container_flatten` function removes singleton containers, but its only caller
is the explicit `split none` command (`sway/sway/tree/container.c:543-556`;
`sway/sway/commands/split.c:34-41`).

A sway 1.11 capture with two headless 1024×768 outputs confirmed the call graph.
After moving both leaves from a stacked container on the left output, GET_TREE
reported two singleton stacked containers on the right output, one for each
leaf. Swayward retains the same wrappers. I3's four top-level child-count
expectations in `524-move.t` therefore do not apply.

This rule is distinct from splitting a singleton horizontal or vertical
container. In that case, sway changes the existing parent layout instead of
creating another container (`sway/sway/tree/container.c:1565-1582`).

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

### Numeric workspace output assignments

Sway treats `workspace <name> output <output>` as a literal-name assignment.
Its parser stores the joined name unchanged, and workspace creation finds the
configuration with `strcmp` (`sway/commands/workspace.c:13-29,135-162`;
`sway/tree/workspace.c:143-174`). Number-prefix lookup exists only in the
separate runtime `workspace number <number>` path
(`sway/commands/workspace.c:190-205`; `sway/tree/workspace.c:493-506`).

I3 instead interprets a bare numeric configuration name as a number assignment,
so `workspace 2 output fake-0` also routes `2:foo`. Swayward follows sway: only
a workspace literally named `2` uses that assignment, while exact assignments
such as `workspace 2:override output fake-1` still apply. A sway 1.11 run with
two headless outputs confirmed that `2:foo` stays on the focused output while a
literal workspace `2` is created on its configured output.

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

The i3 tree creates a distinct `CT_FLOATING_CON` parent when a container becomes
floating. The wrapper is in the workspace's `floating_nodes`, and the selected
container is one level below it in `nodes`. i3 also serializes that distinct type
as `floating_con` (`i3/src/floating.c:232-351,457-460`;
`i3/src/ipc.c:361-383`).

Sway has no floating-wrapper node type. It puts the floating container itself in
the workspace's floating list and serializes that container directly in
`floating_nodes` (`sway/sway/ipc-json.c:478-484,532-540`). Swayward follows
sway. An i3 lookup through `floating_nodes[n].nodes[0]` is therefore one level
too deep in a sway-shaped tree.

The adapter cannot remove this limit. Adding a synthetic parent only for i3
tests would expose a hierarchy that real sway IPC clients never receive. The
underlying behavior can instead be checked against the direct floating node in
a temporary diagnostic or native test. The measured file list and assertion
cost are maintained in [`tests/i3/README.md`](../tests/i3/README.md#floating-wrappers-are-i3-only).

Sway also appends a newly floating container to its list
(`sway/sway/tree/workspace.c:961-971`), while i3 inserts the wrapper at the front
(`i3/src/floating.c:280-295`). Tests that assert both the wrapper depth and i3's
list order need separate classifications for those two differences.

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
merges stay viable.

The [feasibility assessment](specs/2026-09-16-floating-split-container-feasibility.md)
found no failing assertion that this feature alone would turn green. The sole
assertion in `184-regress-float-split-resize.t` already passes vacuously, so promoting it
would also require a native state assertion. Floating split containers remain
deferred until a user-facing requirement justifies the container-forest,
geometry, and lifecycle work.
