# Workspace index and placeholder site mapping

This reference maps the production functions that depend on
`workspaces.len()`, `workspaces.last()`, `workspaces[...]`, or
`active_workspace_idx`. It supplements
[Workspace identity and ordering model](2026-09-16-workspace-order-model.md).

## Inventory scope

The following command finds 184 raw references: 101 in
`src/layout/monitor.rs`, 68 in `src/layout/mod.rs`, and 15 in
`src/layout/tests.rs`.

```sh
rg -n 'workspaces\.len\(\)|workspaces\.last\(\)|workspaces\[|active_workspace_idx' src/layout
```

One `monitor.rs` hit is the `active_workspace_idx` struct field. The other 168
production references belong to 70 file-qualified function definitions. Four
names occur in both production files, so the inventory has 66 distinct function
names. Its 66 names are the rows below. The 15 test references are assertions
and test-driver cursors, not additional production sites.

The table uses these terms:

- **Render position** is a position in the monitor's vertical workspace view.
- **Protocol identity** is a workspace object selected by its stable ID, exact
  name, or parsed number.
- **Lifetime signal** decides whether a workspace exists.
- **Iteration cursor** selects an object temporarily. It does not define that
  object's identity or lifetime.
- **Drain: prevent** means that the function can remove or transfer the last
  workspace from a live monitor. It must create the source replacement first.
- **Drain: observe** means that the function currently assumes a nonempty
  monitor and can encounter a drain caused elsewhere. The completed model makes
  that state unreachable.

The reachability column names the shortest external route. “Bind” includes an
IPC command because sway commands use the same command handlers. “Internal”
means that callers supply the target object or cursor after command resolution.

## Per-function mapping

| Function | Meaning today | Replacement under the settled model | Temporary monitor drain | Reachability |
| --- | --- | --- | --- | --- |
| `Monitor::move_workspace_to_idx` | Render position, plus last-slot lifetime signal | Keep index movement only for native render-order actions. Remove all “last means placeholder” branches. Preserve the active workspace by `WorkspaceId`. Sway protocol ordering is restored by the next create, rename, or reparent sort, not by this native action. | Observe only. It must require a nonempty monitor. | Bind; internal API |
| `Monitor::move_to_workspace` | Render target cursor; the last index also means “creation target” | Existing indices select real workspaces. A move-down request beyond the bottom must create a named next-free workspace before moving the tile. Re-resolve both source and target by `WorkspaceId` after cleanup. | Can create the successor. It must create before detaching content. | Bind |
| `Monitor::verify_invariants` | Lifetime and render invariants | Require a nonempty collection, a valid active cursor, and a name on every attached workspace. Delete both trailing-empty-placeholder assertions. Animation indices remain render positions. | Detects any drain. | Internal test oracle |
| `Monitor::clean_up_workspaces` | Lifetime inferred from active index, last index, name, content, and `persistent` | Replace with the shared sway destruction check: destroy only an empty, output-inactive workspace that no seat retains. Do not unname, preserve a last slot, or consult configuration persistence. Preserve active identity by ID while removing. | Prevent. Never destroy the sole active workspace. | Internal after IPC, bind, close, move, scratchpad, and hotplug transitions |
| `Monitor::move_column_to_workspace` | Render target cursor; last can be the creation target | Resolve an existing target by ID. For a beyond-bottom native move, create and attach a named next-free target before detaching the column. Re-resolve after cleanup. | Can create the successor; create first. | Bind |
| `Monitor::activate_workspace_with_anim_config` | Active render cursor and prior-workspace identity | Keep the render cursor, but require a real named target. Preserve previous workspace by ID and name. After focus changes, run destruction on the old active workspace. | Observe only; target must already exist. | Bind and internal focus paths |
| `Monitor::move_workspace_up` | Native render reordering; last also means placeholder | Keep native render reordering if the action remains supported. Remove placeholder creation. Preserve active and previous workspace IDs. | Observe only. | Bind |
| `Monitor::move_workspace_down` | Native render reordering and implicit creation at the old last slot | Moving past the bottom must first create a named next-free workspace if this action retains create-next behavior. Otherwise swap only existing real workspaces. | Can create a successor; create first. | Bind |
| `Monitor::remove_workspace_by_idx` | Object cursor plus “removing last requires a placeholder” lifetime rule | Accept or resolve a `WorkspaceId`. If the workspace is the source monitor's sole workspace and the monitor remains live, attach a named next-free replacement before removal. Then choose the fallback active workspace by ID. | Prevent; this is a primary drain site. | Internal, chiefly workspace move and hotplug |
| `Monitor::new` | Initial active cursor and unconditional trailing-placeholder construction | Construct the monitor with at least one complete named workspace. If no restored workspace exists, allocate the next-free name before publishing the monitor. Do not append an unnamed object. | Prevent; creates the initial workspace. | Output hotplug/startup |
| `Monitor::move_tiling_subtree_to_workspace` | Source and target object cursors | Unchanged as cursors, but resolve IDs after any creation. Run shared source destruction after attachment and detach completion. | Observe only; moving content does not remove the workspace until cleanup. | IPC and bind |
| `Monitor::move_sticky_to_active_workspace` | Source cursor and active destination cursor | Unchanged as object cursors. Both objects are named real workspaces. Run before destruction of the old source. | Observe only. | Internal focus transition and output handling |
| `Monitor::insert_workspace` | Insertion render position; collection end is reserved for a placeholder | Attach the real workspace, stable-sort the monitor's stored list with sway's comparator, and restore active identity by ID. Remove end clamping and placeholder preparation. | Observe only; destination is already nonempty. | Output hotplug and workspace reparent |
| `Monitor::append_workspaces` | Bulk insertion before a preserved trailing placeholder | Append real workspaces, stable-sort once, and preserve active identity by ID. Do not remove and restore a final element. Empty input remains a no-op. | Observe only; output removal must provide a nonempty receiver. | Output hotplug |
| `Monitor::switch_workspace_up` | Adjacent render position, including the placeholder | Traverse only attached real workspaces. At the top, clamp as the native action does. DnD uses the same real count. | Observe only. | Bind and DnD |
| `Monitor::switch_workspace_down` | Adjacent render position, with the last placeholder as create-next | At the bottom, allocate and attach a named next-free workspace before activation if native focus-down keeps its create-next behavior. DnD clamps to real workspaces and does not create from a preview. | Can create a successor; create first. | Bind and DnD |
| `switch_workspace_auto_back_and_forth` (`Monitor` and `Layout`) | Render cursor plus previous-workspace identity | Resolve the requested and previous workspaces by ID or name. Clamp only after resolving a native index request; protocol requests never derive identity from an index. | Observe only. | IPC and bind |
| `Monitor::insert_new_workspace_at` | Render insertion cursor and active-index repair | Keep as a low-level real-workspace insertion helper only if needed. Update active and animation positions by preserving IDs or applying explicit render offsets. It must not create an unnamed slot. | Observe only. | Internal |
| `Monitor::add_tiling_tile` | Workspace cursor; insertion into last promotes the placeholder | Existing cursor remains. Remove promotion and trailing-slot creation. The caller must create a named target before inserting the tile. | Observe only; reject a missing target internally. | Internal after bind, IPC, map, or drag |
| `Monitor::add_tile` | Workspace cursor; insertion into last promotes the placeholder | Same as `add_tiling_tile`: insert into an existing real target and never change workspace identity. | Observe only. | Internal after bind, IPC, map, scratchpad, or drag |
| `Monitor::active_workspace_idx` | Read-only render cursor | Unchanged as a render cursor. It must never serve as a workspace name or number. | Observe only. | Internal |
| `Monitor::workspaces_render_geo` | Iteration over render positions | Unchanged, because its range is only a render iterator over real workspaces. A separate monitor-owned preview geometry may extend insert hints, but not this workspace collection. | Observe only. | Render and pointer input |
| `Monitor::workspace_render_idx` | Animated render position | Unchanged as render state. Stored-order sorting and removal must remap animation endpoints by workspace identity or cancel the animation. | Observe only. | Render/internal animation |
| `Monitor::update_render_elements` | Existing workspace cursor and prospective insertion position | Existing-workspace lookup stays ID-based. `InsertWorkspace::NewAt` renders only a hint/preview; it must not index a placeholder. Geometry beyond the end is derived from the last real workspace. | Observe only. | Render and drag |
| `unname_workspace` (`Monitor` and `Layout`) | Name removal as a lifetime transition | Remove this runtime transition. Sway rename always supplies another complete name, and destruction removes the workspace object. Configuration removal changes metadata, not a live workspace's name. | Current function can create a monitor with no named workspace; replacement cannot. | IPC/config internals |
| `Monitor::switch_workspace` | Render target cursor | Keep for native index actions, with bounds checked against real workspaces. Protocol activation resolves identity before this call. Post-focus cleanup destroys the old active workspace before command completion. | Observe only. | Bind and IPC |
| `Monitor::sort_sway_workspaces` | Protocol order and active cursor repair | Keep one stable sort using the sway comparator. Preserve active, previous, and animation references by ID. Invoke after create, rename, and reparent only. | Observe only. | Internal after IPC, bind, and hotplug transitions |
| `Monitor::resolve_add_window_target` | Target object cursor | Unchanged as an iteration cursor after every target is a real workspace. `Auto` resolves to the active named workspace. | Observe only. | Window map and internal moves |
| `Monitor::render_above_top_layer` | Active render cursor | Unchanged, because the index selects the active real workspace for rendering. | Observe only. | Render |
| `Monitor::move_to_workspace_up` | Adjacent render target | Keep as a native adjacent-real-workspace operation. It does not create. | Observe only. | Bind |
| `Monitor::move_to_workspace_down` | Adjacent render target and possible placeholder target | If no lower real workspace exists, create a named next-free workspace before moving. Then pass its ID to the common move path. | Can create a successor; create first. | Bind |
| `Monitor::move_column_to_workspace_up` | Adjacent render target | Keep as a native adjacent-real-workspace operation. | Observe only. | Bind |
| `Monitor::move_column_to_workspace_down` | Adjacent render target and possible placeholder target | If no lower real workspace exists, create a named next-free workspace before detaching the column. | Can create a successor; create first. | Bind |
| `Monitor::dnd_scroll_gesture_scroll` | Fractional render position bounded by workspace count | Keep the count of real workspaces. A gesture preview beyond the last workspace requires separate preview state and must not increase this count. | Observe only. | Pointer DnD |
| `Monitor::dnd_scroll_gesture_end` | Render cursor selected by the gesture | Activate an existing real workspace by ID. If future UX allows dropping on a preview, materialize a named workspace before setting the active cursor. | Can create only for a future preview drop; create first. | Pointer DnD |
| `Monitor::dnd_scroll_gesture_begin` | Active render cursor | Unchanged as a render cursor. | Observe only. | Pointer DnD |
| `Monitor::add_workspace_bottom` | Explicit placeholder creation | Replace call sites, not the helper. Native create-next call sites allocate a fully named real workspace. Preview-only call sites allocate no workspace. Remove this anonymous helper. | Current helper can leave only an unnamed workspace; replacement prevents that state. | Internal |
| `add_window` (`Monitor` and `Layout`) | Target cursor; `Monitor` also uses index 0 as a convenient tile factory | Resolve or create the named target before insertion. Window insertion never creates, promotes, names, or reorders a workspace. Replace index 0 tile construction with the resolved target workspace or monitor-level tile construction. | Observe only after target preparation. No-output storage is separate from a live monitor. | Wayland map; rules and IPC-assisted placement |
| `Monitor::add_tile_to_column` | Existing workspace cursor | Unchanged as an iteration cursor. The target column proves that a real workspace already exists. | Observe only. | Internal after drag or move |
| `Monitor::active_workspace_ref` | Active render cursor | Unchanged. The monitor invariant guarantees a real named object. | Observe only. | Internal |
| `active_workspace` (`Monitor` and `Layout`) | Active render cursor | Unchanged as access to the active real workspace. `Layout` still returns `None` only when there are no outputs. | Observe only. | Internal and many bind/IPC queries |
| `Layout::interactive_move_end` | Existing workspace cursor or prospective render insertion position; last means reusable placeholder | Keep existing targets by ID. For `NewAt`, allocate and attach a named next-free workspace before inserting the tile. Keep preview geometry outside `workspaces`; remove index-0 and last-placeholder fallbacks. | Can create a target; create before insertion. It observes hotplug races and must fall back to the active real workspace. | Pointer DnD |
| `Layout::add_output` | Reverse transfer cursor, active-index repair, and placeholder-count heuristics | Gather returning real workspaces by ID. Before the new monitor becomes live, either attach and sort those workspaces or create one named next-free workspace. Preserve the old monitor's active identity by ID and run destruction after transfer. Remove two-workspace and index-1 heuristics. | Prevent on both the new and source monitor; construct first, then transfer, then publish. | Output hotplug |
| `Layout::move_to_output` | Source and destination object cursors | Resolve both workspaces by ID. The destination already has an active named workspace. Attach the tile before source cleanup, then re-resolve target for focus sorting. | Observe only; source cleanup cannot destroy its active sole workspace. | Bind and IPC move commands |
| `Layout::swap_tiling_nodes_between_workspaces` | Borrow-splitting cursors | Unchanged, because indices only obtain two already-resolved workspace objects. | Observe only. | IPC and bind criteria actions |
| `Layout::set_window_sticky` | Source and active-target cursors | Unchanged as object cursors. Move sticky tiles to the active real workspace before considering source destruction. | Observe only. | IPC and bind |
| `Layout::remove_window` | Iteration cursor plus ad hoc lifetime test that protects active and last | Keep the cursor only for lookup. Remove the last-slot exception and call the shared destruction check after tile removal. That check protects the active workspace. | Prevent through shared cleanup; closing the final window on the sole active workspace retains it. | Client close, IPC, and bind |
| `Layout::move_workspace_to_output_by_id` | Source render cursor, source lifetime, and destination insertion position | Change the API to identify the source workspace by `WorkspaceId`. If it is the source monitor's sole workspace, create and attach a named next-free replacement first. Detach, attach, stable-sort the destination, then activate and clean up. | Prevent; this is the main temporary-drain transition. | IPC, bind, and output assignment/hotplug |
| `Layout::move_tiling_subtree_to_node` | Borrow-splitting cursors | Unchanged, because indices only borrow source and target objects already resolved by stable IDs. Run shared source destruction after the complete move when needed. | Observe only. | IPC and bind criteria actions |
| `Layout::move_tiling_subtree_to_sway_workspace` | Protocol target resolved to a storage cursor | Resolve or create the named destination before detach. Carry source and target IDs across mutation, attach first, then run source destruction. | Can create the destination; does not drain a workspace collection directly. | IPC and bind |
| `Layout::refresh` | Iteration cursor compared with active render cursor | Unchanged, because it only marks the active real workspace while refreshing output state. | Observe only. | Internal after commits/config/output changes |
| `Layout::move_column_to_output` | Destination active cursor or explicit cursor, clamped by count | Resolve the destination by ID. The destination monitor invariant supplies an active named workspace. Do not clamp a stale index after sorting. | Observe only. | Bind |
| `Layout::view_offset_gesture_begin` | Iteration cursor compared with a requested or active render cursor | Unchanged, because it selects which existing real workspace receives a horizontal view gesture. | Observe only. | Touchpad/touch input |
| `Layout::update_insert_hint` | Existing workspace cursor | Unchanged as an iteration cursor. A new-workspace hint stays `InsertWorkspace::NewAt` render state and does not allocate until drop. | Observe only. | Pointer DnD |
| `Layout::start_close_animation_for_window` | Existing workspace cursor | Unchanged as an iteration cursor used to start an effect on the workspace that owns the window. | Observe only. | Client close and command close |
| `Layout::should_trigger_focus_follows_mouse_on` | Active render cursor comparison | Unchanged. It compares the pointer target with the active real workspace. | Observe only. | Pointer input |
| `Layout::resolve_sway_workspace_target` | Protocol identity converted to a cursor; absent target reuses the last placeholder | Existing targets return stable IDs internally. An absent target allocates a complete named workspace on the selected output, attaches and sorts it, then returns its ID. Never return a placeholder index. | Can create a destination; create before any caller detaches content. | IPC and bind |
| `Layout::remove_output` | Active identity lookup before removing a monitor | Save the active workspace by ID. The removed monitor may become empty only after it is no longer live. Destroy empty workspaces; move nonempty workspaces only to an already-live destination, then sort. If removing the final output, detached workspace storage may be empty and has no monitor invariant. | Deliberate exception: the removed monitor is unpublished before drain. Destination cannot drain. | Output hotplug |
| `Layout::relative_sway_workspace_position_on_output` | Protocol navigation derived from the active cursor and filtered indices | Traverse the selected output's stored real list directly and wrap. No visibility predicate or placeholder exclusion remains. Return a workspace ID, then resolve its current cursor. | Observe only. | IPC and bind |
| `Layout::move_workspace_to_output` | Active render cursor used as protocol target identity | Capture the active `WorkspaceId` and call the ID-based reparent operation. | Delegates to the drain-preventing reparent transition. | IPC and bind |
| `Layout::find_workspace_by_id` | Iteration cursor returned with the found object | Keep the scan, but callers that mutate ordering must retain the ID and re-resolve the cursor. | Observe only. | Internal |
| `Layout::ensure_sway_workspace` | Absent protocol identity inserted before the last placeholder | Allocate a complete named workspace, attach it to the selected output, stable-sort, and retain configuration separately. If no output exists, keep configuration only rather than creating a runtime workspace. | Can create the first workspace only during output construction; never publish an empty monitor. | Config reload/startup and internal command setup |
| `Layout::advance_animations` | Iteration cursor | Unchanged, because it only advances state in each existing workspace. Deferred destruction runs after an animation through the shared check, not through a last-slot rule. | Observe only. | Internal frame clock |
| `Layout::active_workspace_position` | Active render cursor exported as a temporary location | Prefer returning `WorkspaceId`; convert to a cursor only at the final native index operation. | Observe only. | Internal command resolution |
| `Layout::active_workspace_mut` | Active render cursor | Unchanged as mutable access to the active real workspace. | Observe only. | Internal |
| `Layout::activate_sway_workspace` | Protocol identity lookup; absent identity inserted at the placeholder index | Resolve by exact name or number. For an absent target, create, attach, and sort a named workspace before focus. Focus by ID, then destroy the old inactive empty workspace before returning. | Can create a workspace; target exists before activation and cleanup. | IPC and bind |

## Global relative navigation is not direct stored-order traversal

The current spec claim that global `next` and `prev` traverse one concatenated
stored order is wrong. Only `next_on_output` and `prev_on_output` do that.

Sway parses each workspace's leading nonnegative decimal number with
`workspace_get_number`; a name such as `8:a` therefore belongs to numeric class
8. `workspace_next` and `workspace_prev` scan outputs in root order and scan each
output's stored workspace list, but they use that scan only to select among two
classes (`sway/sway/tree/workspace.c:540-706`):

- From a numbered workspace, `next` selects the globally smallest number greater
  than the current number. `prev` selects the globally greatest smaller number.
- Equal numeric prefixes are not greater or smaller. Because replacement occurs
  only for a strictly better number, the first workspace with the chosen number
  in root-output and per-output order wins.
- If no number exists in the requested direction, navigation crosses to the
  first named workspace for `next` or the last named workspace for `prev`.
- If that class is absent too, navigation wraps to the least or greatest
  numbered workspace.
- From a nonnumeric workspace, navigation follows nonnumeric workspaces in the
  concatenation of root output order and each output's stored order. At either
  end it crosses to the least or greatest numbered workspace, then wraps within
  the available class.
- `next_on_output` and `prev_on_output` are different: they find the current
  object in one output's stored list and increment or decrement that index with
  wrapping.

`cmd_workspace` delegates all four keywords through `workspace_by_name` before
calling `workspace_switch` (`sway/sway/commands/workspace.c:180-238`;
`sway/sway/tree/workspace.c:507-526`). There is no additional command-layer
sort.

This behavior differs from the i3 expectations in
`528-workspace-next-prev-reversed.t` and `535-workspace-next-prev.t`. Those tests
expect all `8:a` through `8:e` to be visited consecutively. Sway's strict
numeric comparison treats all five as number 8, so global navigation does not
walk the five objects as adjacent entries. Swayward must follow sway here. The
three observed `16/22` to `8/30` regressions do not prove that stored sorting is
wrong. They prove that direct stored-order navigation implements i3's oracle
rather than sway's `workspace_next` and `workspace_prev` selection algorithm.
The two vendored files must remain outside the green manifest unless their
portable assertions are separated without modifying the fixtures.

The round-two model remains valid for identity, lifetime, stored sorting, IPC,
and on-output navigation. Its global-navigation consumer rule must be replaced
with the algorithm above.

## Ordered implementation sequence

The implementation must preserve this invariant at every externally observable
and invariant-checked point:

> Every live monitor owns at least one attached, named workspace, and every
> attached workspace is protocol-real.

Use this order:

1. Add the direct stored-order red test from the parent spec. Add focused tests
   for initial output creation, last-workspace reparenting, output removal,
   create-next native actions, and the sway global navigation algorithm. Do not
   commit the tests while red.
2. Introduce one constructor for a complete runtime workspace identity and one
   next-free-name authority. Keep configuration metadata separate. Do not add a
   placeholder or persistent-runtime state.
3. Change `Monitor::new` and `Layout::add_output` first. Build a monitor's
   initial named workspace before inserting the monitor into `MonitorSet`.
   Returning workspaces may replace that initial object only while the monitor
   is still unpublished and only if at least one returned real workspace is
   attached.
4. Add a transaction helper for workspace reparenting. If detaching would drain
   a live source monitor, create and attach its named replacement first. Then
   detach the source workspace, attach it to the destination, stable-sort the
   destination, restore active identities, and publish move/focus changes.
5. Convert explicit protocol creation and native create-next actions. Each path
   creates, names, attaches, and sorts the destination before focus or content
   detachment. Window insertion only inserts a window into that existing
   workspace.
6. Move insert hints and beyond-bottom drag geometry to monitor-owned preview
   state. A preview has geometry and a proposed insertion position, but no
   `Workspace`, `WorkspaceId`, protocol handle, name, or collection entry. A
   drop materializes a real workspace before inserting the tile.
7. Convert content, column, subtree, sticky, scratchpad, close, and interactive
   move paths to carry `WorkspaceId` across mutations. Attach content to the
   destination before running source destruction.
8. Centralize destruction and call it after each complete focus, close, move,
   scratchpad, and output transition. It destroys only an empty inactive
   workspace that no seat retains. It never unnames an attached workspace and
   never destroys the sole active workspace on a live monitor.
9. Replace protocol lookup and global navigation. IPC, ext-workspace, and tree
   serialization iterate the attached collection directly. Implement global
   `next` and `prev` with sway's class-aware scan above; implement on-output
   navigation with direct stored-list wrapping.
10. Remove placeholder branches and index-derived names. Change cross-mutation
    APIs to accept IDs where the table calls for them. Keep indices only for
    rendering, borrow splitting, and local iteration.
11. Add the named-workspace invariant to `Monitor::verify_invariants`, then run
    it after every proptest operation. At this point no transition can expose a
    drain, so the invariant diagnoses an omitted conversion rather than blocking
    the conversion work.
12. Run the focused native and conformance gates, including mutation checks for
    stored order, source replacement before reparenting, and global navigation.
    Run the layout slow gate once because the implementation changes
    `src/layout/`.

This sequence differs from attempt 7 at the drain boundary: it establishes
named output construction and source replacement before removing placeholder
behavior from content paths. It also does not use direct stored-order traversal
for global relative navigation.

## Step 4 is a Layout-level atomic operation, not a Monitor method

The eighth attempt worked this sequence and refused at step 4, for a reason that
is an API defect rather than a design error. Recording it so the ninth attempt
starts past it.

**Step 3 works exactly as specified.** Assigning each new output a next-free
complete identity before `MonitorSet` publication reduced named-workspace
invariant failures from **82 of 90 to 18**. A later partial step 4/5 reached
**8**. That arc — 82, 18, 8 — is the first monotonic progress any attempt has
produced against the invariant.

**Why step 4 cannot live in `Monitor`.** `Monitor::remove_workspace_by_idx`
(`src/layout/monitor.rs:732`) cannot implement a drain-safe transaction:

1. Choosing the replacement name needs a **globally** next-free number, and a
   `Monitor` owns one output. It cannot see the other outputs' names.
2. Its caller passes an **index**, not a `WorkspaceId`, so the operation cannot
   survive the sort that step 4 must perform.
3. Cleanup currently runs **inside** removal, before the destination attachment,
   so the source is already destroyed when the destination needs it.

Attempting to remove the placeholder rules locally produced empty live monitors
and stale inactive-empty workspaces — the monitor drain, arriving through a
different door.

**The shape step 4 must take**, as one atomic `Layout` operation:

1. compute a global next-free identity;
2. if the source monitor holds exactly one workspace, create and attach its
   replacement first;
3. detach the source workspace **by `WorkspaceId`**;
4. attach to the destination and stable-sort it;
5. restore active identities on both sides;
6. only then run destruction on both sides.

This is cheaper than it sounds: `remove_workspace_by_idx` has exactly **one**
non-definition caller in the tree. The same before-detach-after-create ordering
defect affects content moves and interactive drop, which currently detach before
the target exists.

**One warning for the ninth attempt.** Existing native tests assert
placeholder-era counts and indices. They must be updated *after* each
replacement behaviour is green, not before, or they stop being evidence.

## Step 4 is landed

`9618cceb` implemented the atomic reparent at
`Layout::move_workspace_to_output_by_id`, in the specified order: compute a
globally next-free numeric identity from parsed attached names; create and
attach a named replacement first when the source holds exactly one workspace;
detach by `WorkspaceId`; attach and stable-sort the destination; preserve the
destination's active identity when the move is not activating, and the source's
via its replacement; run cleanup on both sides only then. All callers now pass
`WorkspaceId` rather than an index.

This is the first permanent gain the workspace problem has produced in nine
attempts. It keeps every existing test green: 628 passed, `117-workspace.t`
50/50, `132-move-workspace.t` 160/160, `285-sticky.t` 11/11, `505` 90/90, `501`
44/44, slow gate 85/85 at 20,000 cases.

Two honest measurements from that work:

- The named-workspace invariant failed **79 of 90** focused layout tests before
  this change and **79 of 90** after. The reparent path is drain-safe now, but
  the other failures arise before or away from reparenting, so a single step
  cannot move the global count. The focused test
  `moving_the_only_workspace_replaces_it_before_reparenting` is what proves the
  transition, and a mutation disabling the source replacement fails it with an
  empty-monitor index panic.
- The earlier "82 of 90" figure was measured on a different tree state. Current
  main measures **79 of 90**, with 11 passing.

Steps 5 through 8 remain. The same before-detach-after-create ordering defect
still affects content moves and interactive drop.

## The navigation finding, confirmed against live sway 1.11

Measured in a capped headless nested sway 1.11, with `foot` clients so the
workspaces persisted (empty ones are destroyed immediately, which defeats a
naive probe). Stored order was `2(n=2), 5(n=5), 8:a(n=8), 8:b(n=8), 8:c(n=8)`.

    from 8:a:  next -> 2      next -> 5      next -> 8:a    next -> 2
    from 8:b:  prev -> 5      prev -> 2      prev -> 8:c    prev -> 5
    from 8:a:  next_on_output -> 8:b -> 8:c -> 2

This confirms both halves of the mapping's finding and refutes both errors in
the earlier spec:

- **Global `next`/`prev` do not traverse the stored order.** From `8:a`, stored
  order predicts `8:b`; sway goes to `2`. From `8:b`, stored order predicts
  `8:a`; sway goes to `5`.
- **Equal numeric prefixes are skipped entirely.** `8:a`, `8:b` and `8:c` are
  all number 8, and global navigation never moves between them. From `8:a`,
  `next` wraps `2 -> 5 -> 8:a`, visiting exactly one member of class 8.
- **Wrapping selects the least or greatest number**, not a list end: `prev` from
  `2` gives `8:c`, and `next` from `5` gives `8:a` — the first class-8 workspace
  in scan order in each direction.
- **`next_on_output` is different and does walk the stored list**: `8:a -> 8:b
  -> 8:c -> 2`, which is precisely the traversal global `next` does not perform.

So the `8:*` assertions in `528-workspace-next-prev-reversed.t` and
`535-workspace-next-prev.t` are i3-only, as the mapping concluded. They cannot
pass against sway behaviour, and three attempts regressed 16/22 to 8/30 trying.

## Step 5 is landed

`77434134` applied the same ordering to content moves and interactive drop:
named destinations are prepared and sorted **before** any content is detached,
`WorkspaceId` is carried across the sort, and an interactive move's source
workspace survives until the destination attachment has succeeded and is only
then cleaned. Insert-hint and preview representation were deliberately left
alone; that is step 6.

Two mutation proofs, both naming what they got instead:

- Bypassing destination preparation failed
  `move_down_creates_named_destination_before_moving_window` with
  **`None` vs `Some("1")`** — the destination had no name at the moment content
  arrived.
- Eager interactive cleanup failed
  `interactive_move_keeps_source_until_drop_is_attached` because the
  **source ID disappeared** before the drop attached.

Gates held: 638 passed, slow gate 85/85 at 20,000 cases, `117` 50/50, `132`
160/160, `166` 93/4/9, `504` 10/21, `543` 18/45, `285` 11/11, `505` 90/90, `501`
44/44, 242 vendored byte-identical, `passing.txt` 95.

The invariant measured **79 of 90 failures before and 79 of 94 after** — the
denominator moved because this step added four tests, and the count did not,
for the same reason step 4's did not: the remaining failures are in the
placeholder lifecycle, which is steps 6 through 8.

## Step 6 is landed

`d4392881` moved insert hints and beyond-bottom drag geometry to monitor-owned
preview state. A `WorkspacePreview` carries an insertion index and geometry and
holds no `Workspace`, `WorkspaceId`, name or protocol object; hint rendering
consumes the preview geometry, and a drop materialises and sorts a named
workspace before attaching the tile.

The red test was that the old `NewAt` path returned `Rectangle::default` instead
of preview geometry. The mutation bypassed materialisation and failed by
**reusing the placeholder `WorkspaceId`** -- the exact mechanism this step exists
to remove.

**The invariant moved for the first time: 81 of 95 failing before, 80 of 95
after.** Steps 4 and 5 each left the count unchanged because they corrected
ordering rather than identity. This is the first step to retire a placeholder
dependency outright, and it is the first evidence that the remaining count is
reducible rather than structural.

Gates: 645 passed, slow gate 85/85 at 20,000 cases, `117` 50/50, `132` 160/160,
`285` 11/11, `505` 90/90, `501` 44/44, coverage validation still zero.

Steps 7 and 8 remain: carrying `WorkspaceId` across the remaining content paths,
and centralising destruction.

## Step 7 is landed

`abf9bf77` carried `WorkspaceId` through the remaining content paths. Column
moves prepare named destinations and carry the id; cross-output columns attach
before source cleanup; subtree-to-node and sticky paths clean only after
attachment; close and scratchpad detach paths carry the source id across the
mutation.

Mutation: bypassing destination preparation fails with `None` vs `Some("1")`.

Two honest measurements from that work:

- The named-workspace invariant measured **79 of 97 before and 79 of 101
  after**. The denominator grows because each step adds tests. Step 6 remains
  the only step to move the count, because it retired a placeholder dependency
  outright rather than correcting ordering.
- **`503-workspace.t` measures 2 of 18, not the 6 of 18 the entry claimed.** An
  earlier report credited steps 4 and 5 with improving it, and a clean HEAD does
  not reproduce that. The entry is corrected. The claim that dependent files
  gain automatically as the model advances is therefore **unproven**, and it was
  repeated in three briefs before anyone checked it.

Step 8 remains: centralising destruction.

## Step 8 is landed, and the sequence is complete

`d022061b` centralised `WorkspaceId`-based destruction and calls it after focus,
close, content move, sticky, scratchpad, reparent and output cleanup. Sway
authority: `workspace_consider_destroy` at `sway/sway/tree/workspace.c:314-332`,
whose three guards and absent config check are what disproved the fictional
`Persistent` state. The mutation retained the old `WorkspaceId` and failed the
new regression test.

**All eight steps of the sequence are now on main.** Ten attempts were made at
this model; the first nine tried to land it whole and produced nothing
permanent. The five that scoped themselves to a single step all survived.

Two honest measurements:

- The named-workspace invariant measured **79 of 101 before and 80 of 102
  after** -- the same 79 pre-existing failures plus the lifecycle test this step
  deliberately added. So step 6 remains the only step to reduce the count.
- **Every `workspace_model` conformance measurement is unchanged from clean
  HEAD.** `543` still measures 18 of 63. Completing the sequence did not by
  itself convert any conformance assertion.

That second point is the important one, and it is the second time a hoped-for
payoff here has failed to materialise: the earlier claim that `503` improved
once steps 4 and 5 landed was also false. The eight steps removed the
placeholder from the paths they cover and left the model internally consistent,
but the 110 assertions still tagged `workspace_model` need their own
measurement rather than an assumption that this work released them.
