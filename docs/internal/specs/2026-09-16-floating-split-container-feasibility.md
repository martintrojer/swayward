# Floating split-container feasibility

## Problem

Sway can move a whole tiling subtree into a workspace's floating list. The
selected container remains one tree, keeps its internal layout and focus, and
moves or resizes as one floating root. If the workspace itself is selected,
sway first wraps all tiling children. If a child of a floating root is selected,
sway promotes the operation to that root
(`sway/sway/commands/floating.c:23-55`). `container_set_floating` then detaches
or reinserts the selected container as one unit
(`sway/sway/tree/container.c:955-1029`).

Swayward has two different representations:

- `TilingTree<W>` owns nested split and leaf nodes.
- `FloatingSpace<W>` owns `Vec<Tile<W>>`, so every floating root is one window.

`Workspace::toggle_window_floating` consequently accepts one window ID and
moves one `Tile<W>` between those stores (`src/layout/workspace.rs:2038-2125`).
A focused split has no window ID. The non-criteria `floating` command therefore
falls back to the split's active leaf, while the criteria path rejects a
`CommandTarget::Container` (`src/command/mod.rs:377-402` and
`src/command/window.rs:80-102`).

This is a representation gap, not a missing command branch.

## Current measurements

The related vendored files were run individually on `b9d3da38` with the
rejected-command diagnostic enabled. Counts come from `contrib/tap-count`.

| File | Current result | Effect attributable only to floating subtrees |
| --- | ---: | --- |
| `155-floating-split-size.t` | 2 pass, 2 fail | No assertion can be credited. All four depend on unavailable X11-requested 200×80 and 300×90 geometry. A floating subtree alone does not supply that input. |
| `184-regress-float-split-resize.t` | 1 pass | The sole liveness assertion already passes. Replacing both `floating toggle` and `resize grow up ...` with `nop` still produced 1 pass, proving that the assertion does not depend on either operation. Its TAP result cannot change from fail to pass. |
| `185-scratchpad.t` | 2 reached, 2 pass | No unchanged assertion is unlocked. The file stops first at i3's output `content` node. Later sections also depend on the i3 scratch tree, X11 geometry, and restart. |
| `202-scratchpad-criteria.t` | 11 pass before its documented boundary | No unchanged assertion is unlocked. The first blocker is i3's synthetic scratch workspace, not floating-subtree storage. |
| `206-fullscreen-scratchpad.t` | 4 pass, 1 fail before abort | Assertion 3 depends on i3 preserving focus on its extra floating wrapper. A sway-compatible floating subtree has no wrapper and must not claim this assertion. The file later aborts on i3's synthetic scratch tree. |
| `218-regress-floating-split.t` | 2 TAP passes, rejected setup | No assertion is unlocked. It tests that a floating leaf cannot be split, and sway also rejects `layout stacked` for that leaf. Its remaining tree lookup uses i3's permanent floating wrapper. |
| `243-move-to-mark.t` | 11 pass before abort | No unchanged assertion is unlocked. X11 urgency stops the file before its floating-subtree section, and that section also traverses i3's floating wrapper. |
| `291-swap.t` | 11 pass before abort | No unchanged assertion is unlocked. An unavailable X11 client-fullscreen operation stops the file before its floating swap cases; later geometry and wrapper checks remain unavailable. |
| `303-regress-move-floating.t` | 3 pass | Already green. Its current leaf-based sequence proves the assertions without floating-subtree support. |
| `319-gaps.t` | 25 pass, 2 skip, 1 fail | No assertion is unlocked. Assertion 13 follows `layout stacking` on a floating leaf, which sway also rejects. |

The measured unchanged-suite payoff is therefore **zero assertions across zero
files changing from fail to pass**. Assertion 3 of
`206-fullscreen-scratchpad.t` depends on i3 keeping focus on its extra floating
wrapper. Sway has no wrapper and rejects layout changes on a floating
container, so a sway-compatible implementation must not claim that assertion.
`184-regress-float-split-resize.t` is the only possible manifest promotion, but
its one assertion is already green and a direct mutation removed both relevant
operations without making it fail. A native state assertion would be required
before promotion.

The other referenced rows are blocked earlier or independently. Floating split
support does not turn their current failures into passes.

## The i3 floating wrapper is a separate limit

i3 inserts a `CT_FLOATING_CON` parent above the selected container. Sway has no
such wrapper. Sway puts the selected container itself in `floating_nodes`
(`sway/sway/ipc-json.c:478-484,532-540`).

A sway-compatible floating split would therefore serialize as one
`floating_con` root whose `nodes` are the original children. It must not add an
extra parent. The permanent wrapper ceiling remains 35 assertions across 11
files, including wrapper lookups in `218`, `243`, and later `291` sections.
None of those assertions should be credited to this feature.

This distinction also changes how tests must be written. Native tests and direct
sway-shaped `GET_TREE` assertions can prove floating-subtree behavior. An
unchanged i3 assertion that indexes through `floating_nodes[n].nodes[0]` may
still be a cited skip after the behavior exists.

## Types and ownership that would change

A small extension to `FloatingSpace<Tile<W>>` is not sufficient. At least these
models would change:

- **`FloatingSpace<W>` and `Data`:** placement, stacking, activation, movement,
  resize, rendering, hit testing, and animation are per leaf. They must become
  per floating root while still iterating descendant windows.
- **`TilingTree<W>` and `DetachedSubtree<W>`:** detached subtrees preserve shape
  and focus for moves, but `DetachedSubtree` is inert and private. It has no
  resident geometry, rendering, hit-testing, resize, or IPC interface.
  `TilingTree` also assumes one workspace-sized root.
- **`Workspace<W>`:** `floating_is_active` selects a layer, not a node. The
  workspace must track a focused floating root and a focused descendant, route
  focus within a root, and preserve both when roots are raised.
- **`RemovedTile<W>` and `Layout::scratchpad`:** scratchpad storage is leaf-only
  (`VecDeque<RemovedTile<W>>`). A whole floating root needs a removed-container
  form that preserves its subtree, placement, focus, fullscreen state, and
  return slot.
- **`CommandTarget`:** it can name tiling `NodeId`s, but matching and execution
  scan only the tiling tree. Floating roots and descendants need stable
  container identity for floating, resize, move, swap, marks, and scratchpad
  commands.
- **Container marks:** node IDs are currently remapped when a detached subtree
  is attached, and callers migrate `marks_by_container`. A resident floating
  subtree must either keep IDs stable or make every transition migrate marks.
- **`IpcNode` and `src/ipc/tree.rs`:** the recursive type can describe the shape,
  but the workspace serializer currently obtains only one tiling tree and
  constructs every floating node directly from a leaf tile.

The current `toggle_window_floating` and `set_window_floating` APIs have **22
non-test call expressions across 7 source files**, excluding their 4
definitions. Fourteen are callers outside `src/layout`: six command calls, six
input-action calls, and two gesture calls. Eight are forwarding or lifecycle
calls inside `layout/mod.rs` and `layout/workspace.rs`. Tests contain another
**52 direct calls**, including 47 in `src/tests/floating.rs`. Every call that
currently means “this window” needs an explicit decision: preserve leaf
semantics, target the focused container, or promote a descendant to its floating
root.

## Geometry and rendering model

The implementation must not add a second nested layout algorithm to
`FloatingSpace`. `TilingTree::compute_geometry` is already the authority for
split allocations, titlebars, border edges, leaf content rectangles, and IPC
node rectangles (`src/layout/tiling_tree/geometry.rs`). `FloatingSpace::Data`
should remain responsible only for a floating root's outer position and size.

The viable model is a **container forest**:

1. Keep one node arena and one recursive allocation routine.
2. Mark roots as tiling or floating instead of moving leaves into an unrelated
   leaf vector.
3. Give each floating root one placement record and stack position.
4. Run the existing recursive geometry allocator inside that root's assigned
   rectangle, with floating-root gap and decoration rules supplied explicitly.
5. Feed the resulting allocation map to rendering, hit testing, resize, and IPC.

A `Vec<FloatingTree<W>>` could reuse some `TilingTree` code, but it would still
need cross-tree node-ID ownership, mark migration, focus coordination, and
subtree insertion. It also risks making the tiling and floating trees compute
similar geometry under different assumptions. A leaf-or-subtree enum inside the
current `FloatingSpace` is worse: most of its 38 operational methods are
leaf-specific and would grow parallel recursive branches.

The forest is not a localized refactor. It changes the ownership boundary that
most of `Workspace` assumes.

## `GET_TREE` risk

A floating subtree creates a new serializer case:

- the floating root has type `floating_con` and sits directly in the workspace's
  `floating_nodes`;
- descendants remain ordinary `con` nodes;
- every split and leaf needs an absolute `rect` derived from one allocation;
- every descendant `deco_rect` is relative to its parent according to sway's
  layout rules;
- every leaf `window_rect` is relative to that leaf's outer rectangle;
- focus arrays, `percent`, border metadata, fullscreen state, marks, and
  `floating` strings must remain coherent at every depth.

Sway derives these fields from the same pending container boxes used for layout
(`sway/sway/ipc-json.c:532-598,714-762`).
Swayward must likewise extend the allocation map rather than reconstruct nested
rectangles in `src/ipc/tree.rs`. The existing serializer has already shown that
locally plausible rectangle derivations drift from rendering. A new serializer-
only geometry walk would repeat that failure class.

Before implementation, capture real sway trees for horizontal, vertical,
tabbed, and stacked floating roots, including borders and titlebars. Tests must
compare the recursive geometry relationships, not only node presence.

## Required implementation slices

If product demand later justifies the work, split it into independently reviewed
changes:

1. Generalize the tiling node store into a forest while preserving all current
   tiling behavior and IDs.
2. Add floating-root placement and stacking backed by the shared allocation
   map. Do not expose new command behavior yet.
3. Generalize focus, rendering, hit testing, configure sizing, and interactive
   move and resize from a floating leaf to a floating root and its descendants.
4. Generalize detach, reinsert, output moves, scratchpad, swaps, marks,
   fullscreen, close, and map insertion.
5. Change command target resolution so a focused split floats as one unit and a
   child of a floating root promotes to that root, matching sway.
6. Serialize recursive floating roots from the shared allocation map and compare
   them with captured sway fixtures.
7. Add the mutation-proven native scenario needed to make
   `184-regress-float-split-resize.t` evidentiary, then remeasure every file in
   the table above.

Each tree mutation must enter the existing proptest operation model, with
`check_invariants()` after every operation. The implementation would also need
visual coverage because one root now renders several surfaces with shared
movement and resize.

## Scope decision

**Do not implement floating split containers for the current conformance
milestone.** The work is a multi-stage layout ownership rewrite with high focus,
geometry, rendering, scratchpad, and IPC risk. The measured unchanged-suite
benefit is zero fail-to-pass assertions and no newly complete partial file. The
one possible manifest promotion has an existing assertion that remains green
when both floating and resize are removed, so it needs separate native proof.

Keep `184-regress-float-split-resize.t` out of `passing.txt` and remove it from
the gap-only green ceiling. Its unchanged test cannot distinguish a working
floating-subtree resize from the current leaf-only behavior. Retain the broader
feature as a documented sway compatibility gap until a user-facing requirement,
not the conformance score, justifies the container-forest work.
