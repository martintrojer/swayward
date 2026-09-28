# Floating container groups

Status: implemented (option B), pre-beta1
Date: 2026-09-27

Implemented in these commit ranges:

- Step 1: `5aad1943..3ab87f2a`
- Step 2: `9cc7d422`
- Step 3: `0374bf50..c252df99`
- Step 4: `b0bcda97..e05dad6c`
- Step 5: `f3580bc2..4f39b99f`

## Why this is pre-beta1

Sway can float a split container as one object. The container keeps its nested
layout, internal focus history, marks, fullscreen state, and children. Moving or
resizing the floating root moves or reflows the whole group.

Swayward cannot represent that state. `TilingTree<W>` owns nested containers,
while the inherited `FloatingSpace<W>` owns a flat `Vec<Tile<W>>`. Seven command
sites therefore return `floating container groups are not supported` instead of
claiming partial support:

- `src/command/mod.rs`: focused `move scratchpad`, `sticky`, and `floating`
- `src/command/window.rs`: targeted `sticky` and both container paths in
  `floating`
- `src/command/scratchpad.rs`: targeted `move scratchpad`

This is a model gap, not a command-dispatch gap. A focused split has no window
ID that `FloatingSpace` can store. Acting on its focused leaf would destroy the
container boundary that sway preserves.

The earlier feasibility study deferred the feature because the unchanged i3
suite gained no assertions and the necessary ownership change was large. The
product decision has changed: floating a split is part of the sway model and is
now required before beta1. This spec chooses an ownership boundary that can land
in small, reversible steps.

## Sway's model

Sway has one recursive container type. A workspace holds two lists of container
roots: `tiling` and `floating`. Either list may contain a view or a split. There
is no i3-style floating wrapper.

### Floating changes the root's parent, not its contents

`container_set_floating` detaches the selected container and passes the same
container to `workspace_add_floating`. Its children remain attached. Returning
to tiling detaches that same root and inserts it beside the inactive tiling
focus, or at workspace level when no tiling reference exists
(`sway/tree/container.c:955-1050`).

The `floating` command applies these rules before that mutation:

- If the workspace is focused, wrap all tiling children and float the wrapper.
- If any descendant of a floating group is focused, promote the target to the
  group's top-level root.
- Reject an empty workspace and a hidden scratchpad container.

These rules are in `sway/commands/floating.c:23-55`.

`workspace_add_floating` appends any container root, sets the workspace on the
root and every descendant, reparents fullscreen state, and dirties the root and
workspace (`sway/tree/workspace.c:961-973`). `container_is_floating_or_child`
walks to the top-level ancestor and tests whether that root is floating
(`sway/tree/container.c:1343-1354`). Floating is therefore a property of the
root's placement. Descendants do not each become independent floating objects.

### The root rectangle controls the interior layout

A newly floated view first receives the configured default size, then its
natural constrained size and a centered position. A newly floated split has no
natural client size, so the default is 50 percent of the workspace width and 75
percent of its height, clamped by the global floating limits
(`container_floating_set_default_size` and
`container_floating_resize_and_center`, `sway/tree/container.c:847-934`).

Move translates the root and every descendant by the same delta
(`sway/tree/container.c:1074-1159`). Resize changes the root rectangle and calls
`arrange_container`, which recursively allocates that rectangle among the
existing children (`sway/input/seatop_resize_floating.c:36-147` and
`sway/tree/arrange.c:249-262`). The internal split percentages and layout remain
the authority. A group resize does not resize only the leaf under the pointer.

Sway's pointer paths distinguish the two operations:

- A titlebar drag or modifier move promotes a descendant to the top-level
  floating ancestor and moves the whole group.
- Modifier resize also promotes to the top-level ancestor.
- A plain border drag starts from the container whose border was hit. Resizing
  a floating split root reflows its interior. Resizing a descendant changes
  that node inside the group's layout.

See `sway/input/seatop_default.c:346-487`,
`seatop_move_floating.c:38-83`, and `seatop_resize_floating.c:161-198`.

### Focus remains recursive

Sway keeps one seat focus stack for workspaces, container roots, splits, and
views. `seat_get_focus_inactive` returns the most recent descendant of a node.
`seat_get_focus_inactive_floating` accepts any descendant whose top-level
ancestor is floating (`sway/input/seat.c:1361-1413`). Focusing a view in a
floating group therefore records both the active floating root and the path to
that view. `focus parent` and `focus child` continue to walk the group's
interior; clicking a tab or child does not flatten the group.

Raising is root-level. `container_raise_floating` promotes the top-level
ancestor in both the scene and the workspace floating list
(`sway/tree/container.c:1683-1694`). A focus change may occur without raising,
but an explicit activation or pointer operation can raise the whole group.

### Scratchpad, sticky, fullscreen, and moves store roots

The root scratchpad list stores `sway_container *`, not views.
`root_scratchpad_add_container`, `root_scratchpad_show`, and
`root_scratchpad_hide` detach and attach a complete root. They preserve its
children and inactive focus, clear fullscreen where sway requires it, transform
its placement across outputs, and raise the root when shown
(`sway/tree/root.c:99-236`). A child targeted inside a floating group is
promoted to the top-level root before `move scratchpad`
(`sway/commands/move.c:928-950`).

Sticky is also root-level in effect. `container_is_sticky_or_child` tests the
flag on the top-level floating ancestor. Workspace focus moves each sticky root
to the newly active workspace on the same output without changing its interior
(`sway/tree/container.c:1705-1711`, `sway/input/seat.c:1209-1222`, and
`sway/commands/sticky.c:15-49`).

Fullscreen state belongs to a container node and can therefore cover a split or
a leaf. Fullscreen reparents with a floating root. Leaving fullscreen restores
the stored floating rectangle or reinitializes it when no usable rectangle
exists (`sway/tree/container.c:1270-1338`). Swayward must not turn group
fullscreen into independent child fullscreen states.

A move to another workspace detaches and reattaches the same floating root. If
the output changes, sway maps the root's center proportionally from the old
workspace box to the new one. Pointer movement can also change outputs when the
root center crosses to another output (`sway/commands/move.c:198-244` and
`sway/tree/container.c:1100-1159`). Internal layout, focus, marks, scratchpad
membership, sticky state, and fullscreen state travel with the root.

### IPC exposes the same recursive object

A workspace serializes each floating root directly into `floating_nodes` and
then recursively serializes its children into `nodes`
(`sway/ipc-json.c:532-540,859-912`). The root has type `floating_con` and
`floating = "user_on"`; descendants remain ordinary `con` nodes. Hidden
scratchpad roots use the same recursive serializer under the synthetic
`__i3_scratch` workspace (`sway/ipc-json.c:456-486`).

Every node has its own absolute `rect`, parent-relative `deco_rect`, focus list,
percent, marks, border state, fullscreen mode, and scratchpad state. The
serializer uses the arranged container boxes. It does not reconstruct nested
geometry separately for IPC (`sway/ipc-json.c:714-762,810-912`). Swayward must
likewise use the same allocation result for rendering, hit testing, and IPC.

## Design goals

The implementation must satisfy these properties:

1. A workspace floating list contains roots. A root is either one window or a
   nested container tree with one outer rectangle.
2. Moving, raising, changing workspace, changing output, showing from the
   scratchpad, and applying sticky state operate on the root.
3. Interior focus, layout changes, sibling resize, marks, and window lifecycle
   continue to operate on nodes inside that root.
4. Resizing the outer rectangle re-runs the existing tree allocator. It does not
   introduce a second split-layout algorithm.
5. Rendering, hit testing, input regions, pop-up placement, and IPC consume one
   geometry result.
6. Existing single floating-window behavior remains unchanged during the
   migration.
7. Each migration step can ship with the full gate green and can be reverted
   without converting persisted state.

## Options

### Option A: extend the inherited `FloatingSpace`

Under this option, each `FloatingSpace` entry becomes an enum containing either
a `Tile<W>` or a subtree that reuses `tiling_tree`. The existing `Data` record
continues to hold the root position and size. Existing methods branch over the
entry type.

This has one immediate advantage: `Workspace` already delegates floating
position, stacking, rendering, activation, animation, and interactive resize to
`FloatingSpace`. Single-window behavior stays on the old path while subtree
support is added.

The cost is that `src/layout/floating.rs` is inherited niri code built around a
one-entry-one-window assumption. Its storage, active window ID, per-tile
position data, parent-descendant stacking, directional focus, titlebar
rendering, hit testing, configure scheduling, and resize state all encode that
assumption. A subtree enum would put a second recursive branch through most of
this file. Future niri changes to floating placement or animation would then
conflict with swayward's container cases even when the behavior is unrelated.
Every inherited-file edit also needs a `docs/data/divergence.toml` entry, and
`docs/UPSTREAM.md` makes clear that this tree difference is paid again at every
niri release merge.

Option A does preserve more inherited code in the short term, but it does not
preserve a clean upstream seam. It turns `FloatingSpace` into a mixed niri and
sway engine and leaves the hardest behavior split between enum arms.

### Option B: replace it with a swayward-owned floating module

Under this option, a new swayward-owned module stores ordered `FloatingEntry`
roots. An entry contains:

- a single `Tile<W>` or a resident `TilingTree<W>` in subtree-root mode;
- one outer rectangle in workspace coordinates;
- root stacking and active-descendant state;
- root-level move and resize state.

A tree entry's externally visible root is the selected container itself. The
resident-tree adapter must not expose `TilingTree`'s synthetic workspace root or
create an i3 floating wrapper.

The module reuses `TilingTree` for recursive allocation, focus, titlebars,
render order, node identity, marks, and IPC shape. It does not copy the layout
algorithm. The outer module supplies the tree's parent rectangle and translates
its geometry into workspace coordinates. A one-window entry remains a direct
`Tile` initially so the first migration step can preserve behavior exactly. It
may later become a one-leaf tree if that removes code without changing
semantics.

This option costs more up front because the current single-window behavior must
be moved behind a new API before group support starts. It also requires narrow
extensions to `TilingTree`: a resident subtree-root mode, attach and detach that
preserve node IDs and focus history, and geometry that accepts an arbitrary
parent rectangle without applying workspace outer gaps twice.

The long-term boundary is better. The new module is wholly swayward-owned, so
niri can change `src/layout/floating.rs` without forcing sway semantics through
an inherited abstraction. The unavoidable inherited edits are concentrated in
`workspace.rs`, `layout/mod.rs`, `tile.rs`, and input call sites and remain
listed in `docs/data/divergence.toml`. This follows the foundation rule to use a
new module unless the sway model requires an inherited-file edit.

### What must survive replacing `FloatingSpace`

`Workspace` is the only direct production caller of `FloatingSpace`, but it
forwards that state to most compositor features. Replacement parity includes:

- `Tile` rendering, shaders, shadows, blur, rounded corners, focus rings, xray,
  open and close snapshots, and move and resize animations;
- front-to-back iteration for rendering, hit testing, screenshots, screencasts,
  overview and pick-window selection, and input regions;
- parent-child transient ordering, dialog placement, pop-up target rectangles,
  and descendant restacking;
- active-window lookup, directional focus, focus without raise, and activation
  with raise;
- initial size and bounds, client min/max constraints, preset dimensions,
  stored positions, output remapping, and command positioning;
- configure dispatch, transaction throttling, interactive resize hints, and
  move-between-workspaces render layers;
- sticky, scratchpad, fullscreen restoration, urgency, and close lifecycle.

Step 1 treats this list as an equivalence contract. Group support cannot trade
away an inherited niri feature that already works for one floating window.

### Decision

Choose **Option B**.

The feature changes the meaning of a floating entry from a window to a
container root. Owning that boundary is cheaper than maintaining a permanent
subtree branch through niri's leaf-oriented `FloatingSpace`. Option B also
matches sway's model directly: a workspace owns an ordered list of container
roots, each with one outer rectangle.

Do not generalize the whole workspace into a single container forest in this
project. The current `TilingTree` already has tested detach, attach, allocation,
focus, rendering, and IPC logic. Reusing a resident tree per floating group is
the smaller change. Global `NodeId` allocation already avoids collisions. The
module must preserve IDs while a subtree is resident and must migrate container
marks through the existing remap path only when an attach operation cannot
preserve them.

## Boundaries and behavior

### Geometry, rendering, and hit testing

`FloatingEntry` owns only the root rectangle and root motion. For a tree entry,
that rectangle becomes the `TilingTree` parent area. `TilingTree::compute_geometry`
remains the only allocator for split percentages, tabbed and stacked visibility,
titlebars, border edges, leaf boxes, and IPC rectangles.

The floating module applies the entry offset before it exposes geometry to:

- `Tile::update_render_elements` and `Tile::render`;
- `window_under`, titlebar and tab-indicator hits, and border hits;
- pop-up target rectangles and input regions;
- close snapshots, screencast coordinates, and IPC layouts.

The workspace renders tiling roots first and floating roots above them. Within
the floating layer, entries remain back-to-front ordered. Raising any descendant
moves its root entry, not the child, to the front. Fullscreen continues to use
the existing higher render layer and hides ordinary floating roots where sway
does.

A tree entry must not apply workspace outer gaps inside its root rectangle.
Interior gaps and borders still apply according to the nested layout. The outer
rectangle is the floating frame that move and outer resize manipulate.

### Interactive move and resize

A move started on any descendant resolves the containing `FloatingEntry` and
moves its root rectangle. The existing move animation offsets every rendered
leaf by the same root delta. The final position remains unclamped, matching the
current swayward behavior and sway's `container_floating_move_to`.

Outer resize changes the root rectangle and asks the resident tree to recompute
all descendant allocations. Client configure requests use the existing shared
transaction and resize-throttling path. The resize hint propagates to every
participating leaf, as sway's `container_set_resizing` recursively does.

A border on a descendant remains an interior resize. The hit-test result must
therefore identify both the node and the edge; checking only whether a leaf is
floating is insufficient. Modifier resize always chooses an outer root corner.
A plain border resize targets the node whose border was hit.

### Focus and command targets

Each entry tracks a focused node through its resident tree. The workspace also
tracks the active floating entry separately from whether the tiling layer is
active. This replaces `FloatingSpace::active_window_id` without collapsing
focus to a leaf.

`CommandTarget::Container` must resolve across both the workspace tiling tree
and resident floating trees. Commands that address a descendant keep that node
when the command is interior, such as `focus parent`, `layout`, marks, and
internal resize. Commands whose sway semantics are root-level promote the node
to its containing entry: floating toggle, outer move and resize, scratchpad,
sticky movement, and raise.

Focus without raise remains possible. Explicit activation, pointer move or
resize, and scratchpad show can raise the root according to the current sway
paths.

### Dialogs and parent placement

The inherited `FloatingSpace` keeps transient descendants above their parents
and places a new child near or centered over its parent. The replacement must
retain this behavior for ordinary floating windows. A dialog whose parent is a
leaf inside a floating group is a separate floating root, not a new tiling child.
It is centered over the parent leaf's rendered rectangle and stacked above the
parent's root. Raising the parent root also raises its transient root chain in
parent-before-child order.

Pop-ups remain attached to their Wayland parent surface and use that leaf's
geometry. They do not become floating entries.

This keeps file pickers and application dialogs usable without inventing a
container-tree relationship that sway does not have.

### Animations and inherited visual features

The new module keeps `Tile`, so window open, close, resize, shader, shadow,
blur, rounded-corner, focus-ring, and xray behavior remain in the inherited
render path. It must also preserve these `FloatingSpace` behaviors:

- root movement animation and animation threshold;
- open and close snapshots at the final workspace position;
- configure-driven resize animation for each affected leaf;
- titlebar state and urgency;
- move-between-workspaces render layers;
- `deactivate-unfocused-windows` and transaction throttling;
- preset floating width and height for single-window entries;
- directional floating focus and top, bottom, leftmost, and rightmost focus;
- default floating positions and proportional output remapping.

Group movement animates one root offset shared by every descendant. Interior
layout changes use the existing per-leaf resize animation. Preset width and
height commands target the outer rectangle for a group and retain their current
leaf behavior for a single-window entry.

### Scratchpad, sticky, fullscreen, and lifecycle

Scratchpad storage changes from a queue of removed windows to a queue of removed
roots. A stored root carries its tree, root rectangle, focus path, stack rank,
fullscreen state needed for restoration, marks, sticky flag, source area, and
return slot. Showing or hiding never serializes and rebuilds a tree.

Sticky belongs to the entry root. Switching workspaces moves the complete entry
to the active workspace on that output. A sticky child query resolves through
the containing root. Output removal and cross-output movement remap the root
center once and leave interior geometry relative to that rectangle.

Fullscreen may target a split or a leaf inside a floating group. The existing
`TilingTree` fullscreen model remains authoritative for which subtree is shown.
The floating module retains the root rectangle while fullscreen is active and
restores it on exit. Scratchpad transitions clear fullscreen in the same cases
as sway.

Closing a child mutates only its resident tree and runs normal container reaping.
Closing the final child removes the floating entry. Moving the last child out of
a group has the same result. No empty floating root survives an operation.

## Migration plan

Every step ends with nightly formatting, clippy, `cargo test --all`,
`./contrib/check-divergence`, and `./contrib/coverage-report --check`. Changes
under `src/layout/` also run both slow gates from `AGENTS.md`:

- `RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test -p swayward --lib tiling_tree`
- `RUN_SLOW_TESTS=1 PROPTEST_CASES=20000 cargo test --release -p swayward --lib random_operations_dont_panic`

Each step also runs the complete i3 conformance runner and compares the oracle's
random, i3-derived, and event corpora with the previous step. A step may remove
known floating-group mismatches, but it may not add an unrelated regression.

### Step 1: introduce the owned module with leaf entries

**Estimate: 3-5 engineer days.**

Add the swayward-owned floating module with single-window entries only. Port the
current `FloatingSpace` behavior without adding a command or changing IPC. Flip
`Workspace` to the new module in a separate, reviewable commit, then delete the
old module only after equivalence checks pass.

Verification:

- Existing floating, dialog, placement, preset-size, focus, animation,
  fullscreen, sticky, scratchpad, move, resize, and IPC tests remain unchanged
  and green.
- Add model invariants for unique window ownership, nonempty entries, one active
  entry when nonempty, valid transient stacking, and finite root geometry.
- Run all slow and conformance gates listed above.
- Oracle random, i3-derived, and event verdicts show no regressions from the
  pre-step baseline.

Rollback: revert the workspace flip and the new module. No state conversion or
new behavior has shipped.

### Step 2: make a nested tree a floating entry

**Estimate: 5-8 engineer days.**

Add resident `TilingTree` entries and subtree-root geometry. Float and unfloat a
focused split as one root. Preserve node identity, focus history, marks, split
percentages, tabbed and stacked state, borders, and return placement. Route
focus, hit testing, rendering, outer move and resize, internal resize, and
window lifecycle through the owning entry.

This step adds no public command behavior. Tests drive the new layout API
directly until serialization and events are ready. Keeping the refusal avoids a
sway-shaped reply backed by incomplete IPC state.

Verification:

- Extend the layout operation generator with float and unfloat of split nodes,
  root move, outer resize, internal resize, focus parent and child, close child,
  and cross-workspace root moves.
- After every generated operation, check both each resident tree's existing
  invariants and workspace-wide invariants: no empty entry, every node in one
  tree, unique IDs, valid focus paths, sibling percentages summing to one, and
  finite geometry.
- Add headless rendering and hit-test cases for split-h, split-v, tabbed, and
  stacked roots, including root borders and internal borders.
- Run `random_operations_dont_panic`, the complete i3 runner, and all three
  oracle corpora with no unrelated regressions.

Rollback: retain the leaf-entry module from Step 1 and revert nested entry
creation plus command enablement. Existing windows remain representable.

### Step 3: move groups through scratchpad, sticky, workspaces, and outputs

**Estimate: 3-5 engineer days.**

Generalize removed-root storage. Implement group scratchpad show and hide,
sticky workspace following, cross-workspace moves, cross-output placement
mapping, and output-removal evacuation. Preserve internal focus and root stack
order. Integrate group fullscreen save, clear, reparent, and restore behavior.
Keep the corresponding scratchpad and sticky refusal sites in place. The
behavior remains internal until Step 4 can expose it together with correct IPC
and events.

Verification:

- Add generated operations for group scratchpad hide and show, sticky toggle and
  workspace switch, workspace and output moves, output removal, and fullscreen
  before and after each move.
- Assert that hidden roots remain in the scratchpad, visible roots belong to one
  workspace, sticky roots stay on one output, and root-center remapping happens
  once per output change.
- Run focused event-order tests for scratchpad `move`, `floating`, and `focus`
  changes and workspace moves.
- Run the full slow, i3, random, i3-derived, and event gates without regressions.

Rollback: disable group scratchpad and sticky commands, revert removed-root
storage, and keep Step 2's visible nested entries. No on-disk state exists.

### Step 4: serialize groups and emit sway events

**Estimate: 2-3 engineer days.**

Serialize each group root directly in workspace `floating_nodes`, with recursive
children in `nodes`. Serialize hidden groups under `__i3_scratch`. Use the same
geometry allocation as rendering. Emit window and workspace events from the
same mutation phases as the existing leaf paths.

Remove all seven `floating container groups are not supported` refusals only
after the target commands, replies, and events match sway. Enabling the command
paths is the final change in this step, after the model and protocol tests are
green. The count in `src/command` must fall from seven to zero.

Verification:

- Compare recursive `rect`, `deco_rect`, `window_rect`, `percent`, focus, marks,
  border, floating, scratchpad, sticky, and fullscreen fields with pinned sway.
- Check back-to-front `floating_nodes` order separately from focus order.
- Run event scenarios for float, unfloat, raise, scratchpad hide and show,
  sticky workspace following, fullscreen, close child, and cross-output move.
- Run the complete oracle and i3 gates. No sway-shaped envelope may contain a
  flattened or leaf-only approximation.

Rollback: keep internal group behavior but return the structured unsupported
failure from externally incomplete command paths and omit no existing leaf IPC
state. Revert the serializer and event changes as one step.

### Step 5: harden the daily-driver paths

**Estimate: 2-3 engineer days.**

Fix only defects found by the full gates and daily-driver smoke pass. Do not add
new floating features in this step.

The smoke list is explicit:

1. Open an xdg dialog over a tiled parent and over a child in a floating group.
2. Open GTK and Qt file pickers, change focus inside the parent group, and close
   the picker.
3. Run Steam through `xwayland-satellite`; open its transient dialogs and move
   the parent group between outputs.
4. Drag-move a group by a child titlebar and with the floating modifier.
5. Drag-resize every outer edge and corner, then resize an internal split
   boundary.
6. Exercise split-h, split-v, tabbed, and stacked groups with open, close,
   movement, and resize animations enabled and disabled.
7. Send a grouped terminal to the scratchpad, show and hide it repeatedly, move
   it to another output, unfloat it, and confirm its internal focus path.
8. Toggle sticky and switch workspaces on one output, then move the group to a
   second output.
9. Enter and leave leaf and split fullscreen from both visible and scratchpad
   states.
10. Confirm waybar, `swaymsg -t get_tree`, and subscription clients continue to
    parse every reply and event.

Record the compositor log. Any panic or `ERROR` fails the step.

Rollback: revert only hardening changes that regress the gate. If a Step 2-4
contract remains unsafe, restore the relevant structured refusal before beta1
rather than ship partial protocol compliance.

## Oracle completion contract

The feature is done only when a clean oracle worktree shows all of the
following against the pinned sway 1.12 corpus:

- `i3-derived/e5f7d320c0a8b46c` matches. This scenario builds a nested vertical
  and horizontal tree, focuses the parent, and enables floating
  (`155-floating-split-size.t:52`).
- `i3-derived/37f2b2514e87d051` matches. This scenario creates a vertical split,
  focuses its parent, and toggles floating
  (`184-regress-float-split-resize.t:29`).
- `i3-derived/48cbb00b74e3b7fe` matches. This scenario floats a parent, focuses a
  child, and moves within the group (`303-regress-move-floating.t:26`).
- The random corpus's focused set of about ten seeds matches for sequences that
  expose parent focus or root scratchpad movement. Start with seeds `77`, `82`,
  `84`, `97`, `98`, `111`, `219`, `290`, `300`, and `327`; replace a seed only
  when its mismatch is proven unrelated, and record that reason in the task
  note.
- New event scenarios for grouped float, scratchpad, sticky, fullscreen, raise,
  and workspace or output move match in payload and order.
- `rg -n 'floating container groups are not supported' src/command` returns no
  matches. The current baseline is seven sites.
- The complete random, i3-derived, and event corpora have no new mismatch
  outside the scenarios attributed to this feature.

The three i3-derived scenarios are sway-shaped IPC evidence, not a promise that
all unchanged i3 wrapper assertions become valid. Sway has no i3 floating
wrapper, so wrapper-dependent i3 assertions remain cited skips.

## Risks

### Geometry can diverge between screen and IPC

A locally plausible serializer can report rectangles different from those used
for rendering. The mitigation is structural: one `TilingTree` allocation feeds
rendering, hit testing, pop-up placement, and IPC. Recursive rectangle fixtures
must cover every layout.

### Group resize can deadlock configure transactions

One outer resize can configure several clients. A slow or nonresponsive child
must not block the session indefinitely or produce a mixed old/new layout. Reuse
the existing shared configure intent and transaction blockers, test with one
child withholding a commit, and preserve the current resize-throttling escape
hatches.

### Focus and raise can collapse into one operation

The inherited floating model often activates by moving a window in the stack,
while sway can focus without raising. The owned module keeps active entry,
focused node, and stack order as separate state. Tests must cover pointer focus,
command focus, explicit raise, and focus restoration after close.

### Node IDs and marks can be lost at tree boundaries

Current detached subtree attachment may remap IDs and requires callers to move
container marks. A resident floating tree must preserve IDs for its lifetime.
Every transition test records node IDs and marks before and after floating,
scratchpad, workspace, and output moves.

### Dialog stacking can regress ordinary applications

Replacing `FloatingSpace` risks losing niri's transient ordering and parent
placement even before nested roots are enabled. Step 1 keeps this behavior and
blocks later work on dialog and file-picker smoke tests.

### Fullscreen has two rectangles

A floating group needs both its stored root rectangle and its active fullscreen
allocation. Conflating them loses placement on exit or moves the wrong subtree.
The model keeps the root rectangle while `TilingTree` owns the fullscreen node.
Generated tests combine fullscreen with every root transition.

### Upstream merge cost moves rather than disappears

Option B removes long-term subtree branches from inherited `floating.rs`, but
`Workspace`, input routing, `Tile`, and layout forwarding still change. Every
inherited-file edit gets a precise `docs/data/divergence.toml` entry. New logic
stays in the owned module so later niri release merges see narrow adapters
instead of sway semantics spread through niri's floating engine.

### The migration can become one unreviewable rewrite

The safeguard is the leaf-only flip in Step 1 and the refusal boundary between
steps. No step enables a command before the model, tests, IPC, and events needed
for that command are present. Each step has a direct revert path.

## Scope

### In scope

- Floating split-h, split-v, tabbed, and stacked container roots
- Workspace-root wrapping for `floating enable`
- Root promotion when a descendant is targeted
- Recursive focus, layout, rendering, hit testing, move, resize, and lifecycle
- Scratchpad, sticky, fullscreen, workspace and output moves
- Sway-shaped recursive `floating_nodes` and event payloads
- Existing niri visual effects and animations on every descendant `Tile`

### Out of scope

- i3's extra `CT_FLOATING_CON` wrapper and wrapper-shaped assertions
- A general rewrite of tiling and floating storage into one workspace forest
- New IPC fields or swayward-only floating commands
- Persisting layout state across compositor restarts
- Changing sway's no-clamp floating position behavior
- New animation types; this work preserves the inherited set

## Open questions

There are no product questions blocking implementation. Two implementation
choices must be settled in Step 2 with executable evidence:

- Whether a one-window entry remains a direct `Tile` or becomes a one-leaf
  `TilingTree` after the migration. Keep the direct form unless unifying it
  removes more branching than it adds.
- Whether subtree attach can preserve all `NodeId`s. Prefer preservation. If the
  current arena API requires remapping, make the remap explicit and migrate
  marks, command targets, and focus references in one transaction.

Neither choice changes the external model or the migration order.
