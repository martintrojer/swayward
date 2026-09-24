# Workspace identity and ordering model

## Problem

Sway stores workspaces in a sorted list on each output. It sorts after creation and after reparenting (`sway/sway/tree/workspace.c:255-259` and `sway/sway/tree/output.c:247,387-405`). Numeric names sort by their leading base-10 number. Numeric workspaces precede nonnumeric workspaces. Nonnumeric names compare equal, so the stable sort preserves their insertion order on that output.

Swayward instead stores niri's workspace-switching slots in each `Monitor::workspaces` vector. The final unnamed empty slot is an internal window-creation target. It is not a sway workspace, but its current vector index supplies a fallback name when the active empty slot is reported. Workspace identity, retention, rendering position, and sway ordering are therefore coupled.

Sorting only `GET_WORKSPACES` or `GET_TREE` cannot fix this difference. Sway's navigation traverses the stored sorted lists. A serialization-only sort would report the expected order while `workspace next`, `workspace prev`, cleanup, and later insertion still used a different model.

## Why the two previous attempts failed

The first attempt sorted `Monitor::workspaces` directly. When the sort moved the trailing unnamed slot, its index-derived fallback name changed. IPC then exposed workspaces `4` and `7` that the test had never created, breaking assertions 33 and 41 in `117-workspace.t`. Keeping the unnamed slot at the end prevented those phantom names, but the occupied workspace that had originated as an unnamed slot still lacked a stable numeric identity. It sorted with named workspaces and changed the `next` and `prev` results checked by assertions 11 and 15. The attempt treated a rendering slot as a sway workspace without first separating their identities.

The second attempt introduced one `is_sway_visible` predicate for IPC, relative navigation, cleanup, and assignment. Those consumers do not have one membership rule. IPC and navigation must include an active empty real workspace. Cleanup must remove an inactive empty transient workspace, but retain an empty configured workspace. The candidate predicate either retained stale transient names and broke assertion 2 in `117-workspace.t`, or removed inactive configured workspaces. Materializing an implicit workspace's number through `set_sway_identity` did not solve that problem because the same fields also changed cleanup behavior. The attempt conflated protocol identity with lifetime.

## Superseded proposed model

The round-two analysis below supersedes this model. In particular, sway has no
placeholder workspace and no persistent runtime-workspace lifecycle. This
section remains as the record of the model tested in the fourth experiment.

### Stable identity

Every real sway workspace has an identity independent of its vector position:

- `name`: the exact workspace name that IPC and commands use.
- `num`: the leading decimal number, or `-1` for a nonnumeric name.

The internal creation placeholder has no sway identity. An occupied or activated implicit slot must receive a real identity before any insertion sort. Its number comes from sway's next-free-number rule, not from its later vector index.

The existing `WorkspaceId` remains the internal object ID. It is not the sway name or number and does not determine ordering.

### Lifecycle

Identity and retention must be separate state. At minimum, a workspace is one of:

- `Placeholder`: the internal trailing creation target. It has no sway identity and is never exposed to sway consumers.
- `Transient`: a real sway workspace. It survives while active or nonempty and is destroyed after it becomes both inactive and empty.
- `Persistent`: a configured real sway workspace. It survives while empty and inactive.

Renaming changes identity, not lifecycle. Promoting the placeholder creates a transient real workspace and a new trailing placeholder. Initial output creation likewise creates a real active workspace, using the configured binding name or the next free number, plus a separate placeholder.

### Stored order

`Monitor::workspaces` remains the source of truth for rendering and navigation, but it has two regions:

1. a prefix of real workspaces in sway order;
2. the internal placeholder at the end.

Creation and reparenting insert a real workspace into the prefix and stably sort that prefix with sway's comparator:

- compare two numeric names by the leading number;
- place numeric names before nonnumeric names;
- compare two nonnumeric names equal.

The sort must preserve the active workspace by `WorkspaceId`, then recompute `active_workspace_idx`. Reparenting appends the moved workspace to the destination's real prefix before the stable sort. This preserves the destination's existing nonnumeric order and places the moved nonnumeric workspace after it, as sway does.

### Consumer collections

- **IPC (`GET_WORKSPACES` and `GET_TREE`)** reads the real prefix. It includes active empty, transient nonempty, and persistent workspaces. It never reads the placeholder.
- **Relative navigation** reads the stored real prefixes. `next_on_output` and `prev_on_output` traverse one output's prefix. Global `next` and `prev` traverse outputs using the existing global output traversal, but use each output's stored real order. They never visit the placeholder.
- **Cleanup** examines lifecycle, activity, and contents. It destroys only a transient real workspace that is inactive and empty. It retains persistent workspaces and maintains exactly one placeholder per output. Cleanup does not infer lifecycle from `name`, `num`, or vector position.
- **Creation** is the only consumer of the placeholder. An explicit workspace command creates a transient real workspace with the requested identity. Native window creation or activation through the placeholder promotes it to a transient real workspace with a stable next-free identity, sorts the real prefix, and appends a new placeholder.

Output assignment and workspace moves operate on real workspace identity. After a move, both source cleanup and destination sorting run against the model above.

## Scope decision

The model is coherent, but it is not safely separable into an ordering-only patch. The first valid production change must add distinct identity and lifecycle state and update creation, cleanup, navigation, output movement, and IPC together. Changing only sorting, visibility, or serialization repeats one of the reverted failures. No production implementation is included in this analysis commit.

## Ordered output fallback assignments share this dependency

Sway stores `workspace <literal-name> output <output>...` as configuration
metadata. The directive does not create a workspace. When sway later creates a
workspace, `workspace_find_config` compares the complete name with `strcmp`, and
`workspace_get_initial_output` selects the first configured output that
currently exists. If none exists, creation uses the focused output. The output
lookup accepts either a connector name or an output identifier
(`sway/sway/tree/workspace.c:143-182`; `sway/sway/desktop/output.c:42-63`).

This lookup is small, but swayward cannot add it as an isolated
`Option<String>`-to-list change. Its `Workspace` KDL node is also the persistent
workspace object: `Layout::with_options_and_workspaces` eagerly creates every
configured entry before outputs are added. Translating an assignment into that
node therefore creates a workspace that sway would create only on demand. In
`522-rename-assigned-workspace.t`, removing only the two fallback directives
reaches seven assertions; two already fail because assigned destination names
exist eagerly and make rename reject them. Extending the output field would
remove the parser refusal while preserving the wrong lifecycle.

Exact hotplug behavior also needs the configured priority list to survive after
workspace creation. Sway moves an existing workspace when a newly available
output becomes its highest-priority configured output, and uses the same list
when an output disappears (`sway/sway/tree/output.c:35-49,213-247`;
`sway/sway/tree/workspace.c:770-817`). A creation-only lookup would pass the
simplest startup case but silently lose this runtime behavior.

The implementation must therefore add assignment metadata that is separate from
persistent workspace declarations, use literal-name lookup, resolve names and
identifiers in order at creation time, retain priorities for output hotplug and
removal, and fall back to the focused output when no configured output exists.
It must not add i3's numeric-prefix matching. This work belongs with the identity
and lifecycle model above. Until then, `contrib/sway-to-kdl` must continue to
reject ordered fallback lists instead of keeping only their first entry.

Output removal now implements the separable lifecycle behavior from sway's
`output_evacuate`: empty workspaces on the removed output are destroyed, while
sticky-only contents move to the surviving output's active workspace
(`sway/sway/tree/output.c:205-247`). Stored output-priority selection and sorting
the receiver remain deferred because both require this document's complete
identity, ordering, and assignment model.

## Current measurements

Measured on `90b2f198` with the rejected-command diagnostic enabled:

| File | Result | Rejected commands |
| --- | ---: | ---: |
| `503-workspace.t` | 2 pass, 16 fail | 0 |
| `528-workspace-next-prev-reversed.t` | 16 pass, 22 fail | 0 |
| `535-workspace-next-prev.t` | 16 pass, 22 fail | 0 |
| `139-ws-numbers.t` | 3 pass, 5 fail | 0 |
| `515-create-workspace.t` | 1 pass, 1 fail | 0 |

The first assertion in `515-create-workspace.t` now passes because the initial workspace can take its name from a default-mode binding. The remaining assertion still exposes the missing next-free-number lifecycle. As an additional check, `514-ipc-workspace-multi-monitor.t` remains at 2 pass and 1 fail with no rejected commands.

## Implementation prerequisites

A later implementation needs one mutation-proven layout test that inspects the stored per-output sequence directly. The test must create numeric workspaces out of order, create several nonnumeric workspaces, and reparent both kinds. It must fail if sorting is moved to IPC serialization. Only after that red test should production state change.

The implementation must then run the focused conformance files above and the workspace regression gates named in `AGENTS.md`, followed by the required 20,000-case `tiling_tree` slow gate.

## Prerequisite experiment

A direct stored-order test was written against `Layout` after creating numeric
workspaces `10`, `2`, `8`, and `3`; creating nonnumeric workspaces `alpha`,
`beta`, `gamma`, `delta`, and `epsilon`; and moving both `alpha` and `2` to the
second output. The expected destination sequence was `2`, `3`, `8`, `delta`,
`epsilon`, `alpha`, followed by the internal placeholder. The current stored
sequence was `8`, `delta`, `3`, `epsilon`, `2`, `alpha`, followed by the
placeholder.

The test established the required direct-state seam and failed before any
production edit. Making it green requires sorting creation and reparenting.
Doing that without the identity and lifecycle changes described above would
repeat the first reverted attempt. The test was therefore removed rather than
leaving main red, and no production change was made.

## Identity-only separation experiment

A narrower experiment removed all four `index + 1` fallbacks from
`src/ipc/tree.rs`, so an anonymous workspace serialized as `name: ""` and
`num: -1` regardless of its vector position. The direct regression test passed:
moving an occupied anonymous workspace no longer changed its reported number.
However, the unchanged `117-workspace.t` regressed from 50 reached and 50 passed
to 48 passed and 2 failed. Assertions 11 and 15 expected the active anonymous
slot to remain workspace `1` during relative navigation, but it had no real
identity to preserve.

Assigning a number only in the serializer would replace a position-derived
identity with serializer-owned identity and repeat the same coupling. Assigning
one when the slot becomes active or occupied must also create a new trailing
placeholder, choose the next free number globally, and update navigation and
cleanup. That is the complete lifecycle transition described above, not a safe
separation-only change. The `index + 1` fallbacks therefore remain until that
model lands.

The lifecycle half is independently separable. `set_sway_identity` now changes
only `name` and `number`; it no longer clears `persistent`. A regression test
renames an empty configured workspace, switches away, runs cleanup, and verifies
that the renamed workspace remains present. This preserves the existing boolean
model without claiming to implement the three-state lifecycle.

## Whole-model experiment and fourth refusal

Measured on `262f14ef`. The complete model described above was implemented:
the three-state lifecycle enum, the real-prefix stable numeric sort, initial
next-free identity, creation and reparent sorting, and the removal of all four
`src/ipc/tree.rs` index fallbacks. The spec's own prerequisite red test was
written first and failed before any production edit, then passed.

Five files improved:

| File | Before | After |
| --- | ---: | ---: |
| `139-ws-numbers.t` | 3 pass, 5 fail | 8 pass, 0 fail |
| `503-workspace.t` | 2 pass, 16 fail | 14 pass, 4 fail |
| `506-focus-right.t` | 25 pass, 6 fail | 31 pass, 0 fail |
| `514-ipc-workspace-multi-monitor.t` | 2 pass, 1 fail | 3 pass, 0 fail |
| `519-mouse-warping.t` | 1 pass, 2 fail | 3 pass, 0 fail |

Four regressed, including the mandatory canary:

| File | Before | After |
| --- | ---: | ---: |
| `117-workspace.t` | 50 pass, 0 fail | 49 pass, 1 fail (assertion 2) |
| `166-assign.t` | 98 pass | 93 pass, 4 skip, 9 fail |
| `528-workspace-next-prev-reversed.t` | 16 pass, 22 fail | 8 pass, 30 fail |
| `535-workspace-next-prev.t` | 16 pass, 22 fail | 8 pass, 30 fail |

`304-ipc-workspace-init.t` also worsened from 6 pass, 3 fail to 2 pass, 7 fail.
`132-move-workspace.t` held at 160. The work was reset rather than landed
partially, because landing only the sort or only the fallback removal repeats
one of the first three documented failures.

The canary failure is the diagnostic one. Assertion 2 retained an old transient
workspace, so the model creates protocol-real identity correctly but does not
retire an active-empty implicit workspace when focus leaves it. The three
navigation files regressing together (`528`, `535`, `304`) points at the same
seam from the other side: relative navigation now traverses a real prefix whose
membership rules were specified for IPC, not for navigation. This is the second
attempt to be defeated by the same distinction the "Consumer collections"
section warned about, which suggests the section names the consumers correctly
but does not specify their *transitions*.

### What a corrected design must specify

1. When an initial output workspace becomes protocol-real versus a placeholder.
2. How switching away retires an active-empty implicit workspace without
   violating active-empty IPC visibility, which assertion 2 of `117-workspace.t`
   and the IPC consumer rule currently demand in opposite directions.
3. Event-state membership and identity across create, activate, move, and
   cleanup, as transitions rather than as per-consumer predicates.

Until those three are specified, the model above is not implementable. The
measurements here are the cost of finding that out: five files' worth of real
progress is available, but only behind a correct answer to item 2.

## Round-two model: every workspace is real

### Real sway has no placeholder workspace

Sway does not create an anonymous workspace and later promote it. Every
`sway_workspace` has a non-null name at construction. `workspace_create` creates
the ext-workspace handle, attaches the workspace to an output, sorts the output's
workspace list, publishes its name and group, and emits `workspace::init`
(`sway/sway/tree/workspace.c:177-269`). When an enabled output has no restored
workspace, `output_enable` chooses `workspace_next_name`, creates that real
workspace, and focuses it before any client window exists
(`sway/sway/tree/output.c:150-175`).

A capped sway 1.11 headless run confirmed the source behavior. The run used one
headless output and queried `GET_WORKSPACES` at each state:

| State | `GET_WORKSPACES` names |
| --- | --- |
| Fresh output, before any window | `1` |
| After a real Wayland window opened | `1` |
| After that window closed, while the workspace remained active | `1` |
| After `workspace 2` moved focus away | `2` |

The initial workspace is therefore protocol-real before it contains a window.
Opening a window does not promote or rename anything. The empty workspace stays visible while it is active on its output. When
another workspace becomes active on that same output, sway destroys the old
empty workspace. Moving seat focus to another output does not retire it because
it remains that output's visible workspace.

`GET_WORKSPACES` does not apply a visibility predicate to a larger internal
workspace set. It iterates every workspace in the root and serializes each one
(`sway/sway/ipc-server.c:585-602,719-727`). `GET_TREE` likewise serializes every
workspace attached to each output (`sway/sway/ipc-json.c:854-874`). The public
workspace collection and the compositor's workspace collection are the same
set.

Swayward's trailing unnamed workspace is a niri rendering and navigation
artifact. It must not be promoted into sway identity and must not remain a
`Workspace` beside real sway workspaces. The corrected model removes it from
the workspace collection. An empty output is rendered by its active, named real
workspace. If an inherited gesture needs an uncommitted preview beyond the last
workspace, that preview belongs to gesture state, not to `Monitor::workspaces`,
IPC, ext-workspace, lookup, sorting, or navigation.

### Active-empty visibility is a state, not a lifetime promise

Assertion 2 of `117-workspace.t` is correct. The earlier statement that it
conflicts with active-empty IPC visibility is wrong because it compares two
successive states as if they were simultaneous.

Sway first changes focus to the destination workspace. It then calls
`workspace_consider_destroy` on the workspace that was active on that output
before the focus change (`sway/sway/input/seat.c:1158-1165,1194-1206,1243-1245`).
The destroy check retains a workspace while it is the output's active workspace,
but destroys it when it is empty, inactive, and absent from every seat's
inactive focus (`sway/sway/tree/workspace.c:299-331`). Destruction emits
`workspace::empty`, destroys the ext-workspace handle, and detaches the
workspace from its output before later IPC queries can enumerate it
(`sway/sway/tree/workspace.c:299-311`).

The corrected rule is:

> An output-active empty workspace is real and visible. A transition that makes
> another workspace active on that output must destroy the old workspace before
> the transition completes if it remains empty and no seat retains it.

This rule satisfies both observations. Before the switch, IPC reports the active
empty workspace. After the switch, IPC does not report the old workspace.
There is no intermediate stable state in which an inactive empty workspace must
remain visible.

Configuration does not add a persistent runtime-workspace class. Sway consults
workspace configuration while creating a workspace, but
`workspace_consider_destroy` does not consult configuration
(`sway/sway/tree/workspace.c:226-253,314-331`). Workspace assignment and gaps are
metadata for creation and placement, not reasons to retain an empty inactive
workspace. The earlier `Persistent` lifecycle state was therefore another spec
defect.

### Workspace transitions

The implementation must use one attached-workspace collection for lookup,
stored order, navigation, IPC, and ext-workspace. The transitions below mutate
that collection. They do not ask each consumer to reconstruct membership.

| Transition | Required state change | Required publication and cleanup |
| --- | --- | --- |
| Enable an output with no restored workspace | Choose a name with the next-name rule, create a real workspace with that complete identity, attach it, and sort the output list. | Publish ext-workspace and `workspace::init`, then focus it if the seat lacks focus. There is no placeholder (`sway/sway/tree/output.c:150-175`; `sway/sway/tree/workspace.c:177-269,436-490`). |
| Create and activate an absent workspace | Resolve the complete requested name, create the real workspace on its selected output, attach it, and sort immediately. Then focus it. | Emit `init` during creation. Emit `focus` during the focus transition. After focus changes, destroy the destination output's previous active workspace if it is empty and no seat retains it (`sway/sway/commands/workspace.c:180-238`; `sway/sway/input/seat.c:1158-1245`). |
| Activate an existing workspace | Keep its identity and stored position. Change output and seat focus to the existing object. | Emit `focus`. Then apply the same cleanup to the destination output's previous active workspace. A cross-output focus change leaves the source output's visible workspace active. Switching to the already focused object is a no-op and must not destroy it (`sway/sway/tree/workspace.c:731-742`; `sway/sway/input/seat.c:1130-1165,1194-1245`; `sway/sway/desktop/output.c:76-85`). |
| Open a window | Insert the view into the already active real workspace. | Do not create, promote, rename, or reorder a workspace. The fresh-output measurement shows that identity predates the first window. |
| Move content between workspaces | Resolve or create the real destination before detaching content. Move the content, then consider the emptied source for destruction. | Keep both workspace identities unchanged. Destroy the source only if it is now empty and inactive. Sway invokes this cleanup from its move paths (`sway/sway/commands/move.c:459-614,724`). |
| Move a workspace between outputs | Detach and reattach the same real workspace. If the source would have no workspace, create a new named real workspace there. Sort the destination after attachment. | Consider the destination's displaced active workspace for destruction, preserve the moved workspace's identity, and emit `workspace::move` (`sway/sway/tree/workspace.c:1131-1160`). |
| Disable an output | For each workspace, destroy it if it is empty apart from sticky windows. Otherwise move it to its highest-priority available output and sort the receiver. | Emit `empty` for destruction or `move` for reattachment. Do not transfer an empty placeholder because none exists (`sway/sway/tree/output.c:205-255`). |
| Finish any focus, close, move, or scratchpad transition | Run the shared destruction check on each workspace that may have become empty and inactive. | Remove the workspace object and its protocol handle once. IPC and relative navigation then observe the updated collection (`sway/sway/tree/workspace.c:299-331`; call sites in `sway/sway/input/seat.c:1243-1245`, `sway/sway/tree/view.c:988-998`, and `sway/sway/commands/move.c:614,724`). |

Relative navigation uses the real collection after each transition. Global
`next` and `prev` implement sway's numeric-first and named-workspace traversal
over all output lists (`sway/sway/tree/workspace.c:540-677`). `next_on_output`
and `prev_on_output` wrap directly through the selected output's stored list
(`sway/sway/tree/workspace.c:680-706`). Because cleanup finishes before the next
command, navigation never needs an IPC-specific membership predicate and never
sees a retired empty workspace.

### Falsifiable `117-workspace.t` predictions

The current baseline reaches 50 assertions and passes all 50. The round-two
model must preserve these five canaries:

| Assertion | Prediction | Reason |
| ---: | --- | --- |
| 2 | Pass | Creating and focusing `$otmp` makes the empty `$tmp` inactive. The focus transition destroys `$tmp` before `workspace_exists` queries IPC (`sway/sway/input/seat.c:1158-1245`; `sway/sway/tree/workspace.c:314-331`). |
| 11 | Pass, focused workspace `1` | At this point the real ordered set is numeric `1`, then the two nonnumeric temporary workspaces in insertion order. `next` from the second named workspace wraps to the least numbered workspace (`sway/sway/tree/workspace.c:613-677`). |
| 15 | Pass, focused workspace `1` | `prev` from the first named workspace falls back to the greatest numbered workspace, which is `1` (`sway/sway/tree/workspace.c:548-610`). |
| 33 | Pass, workspace `4` absent | `4: foo` is a distinct real identity. No anonymous slot derives the number `4` from its vector index (`sway/sway/tree/workspace.c:493-505`; `tests/i3/t/117-workspace.t:149-163`). |
| 41 | Pass, workspace `7` absent | `7: foo` remains the only workspace with numeric prefix 7 until addressed by `workspace number 7: bar`; no placeholder can serialize as `7` (`sway/sway/tree/workspace.c:493-505`; `tests/i3/t/117-workspace.t:175-191`). |

These predictions are falsifiable at the unchanged i3 test boundary. Any model
that fails assertion 2 has delayed cleanup past the command transition. Any
model that fails assertions 11 or 15 has changed real-workspace ordering or
navigation. Any model that fails assertions 33 or 41 still derives protocol
identity from storage position.

### Revised implementation boundary

Do not implement the earlier three-state `Placeholder`, `Transient`, and
`Persistent` lifecycle enum. Sway's runtime model needs none of those states.
A workspace is either an attached real workspace or it has entered destruction.
Its exact name is always present while attached.

The first implementation must remove the trailing unnamed `Workspace` invariant
and replace every operation that depended on it:

1. Create a named real workspace when an output would otherwise have none.
2. Make explicit workspace creation allocate and insert a new object rather than
   renaming or promoting a slot.
3. Make inherited create-next actions allocate a real next-free workspace before
   activation. Keep gesture-only preview outside the workspace collection.
4. Centralize the post-transition destruction check and call it after focus,
   close, content move, workspace move, scratchpad, and output transitions.
5. Keep the attached collection sorted at creation, rename, and reparent time.
6. Let IPC, ext-workspace, lookup, and navigation consume that collection
   directly, with no visibility predicate and no index-derived fallback name.
7. Store workspace configuration and output priorities separately from runtime
   workspace existence. Configuration must not retain an empty inactive object.

This is still a whole-model change, but it has one fewer state machine than the
fourth attempt. The key replacement is not a better placeholder predicate. It is
removing the placeholder from workspace identity, lifetime, and storage.

### Two blockers found by the first implementation attempt

A fifth implementation pass reset cleanly rather than land a partial model. It
observed the prerequisite red (stored `[epsilon, delta, 3, 8, 2, alpha]` against
expected `[2, 3, 8, delta, epsilon, alpha]`, a different wrong order from the
one recorded above, so the stored order is not stable across changes) and then
named two costs that the seven steps understate:

1. **Render and gesture.** Niri's gestures are why the trailing workspace exists,
   so removing it from the collection reaches the renderer. The seven steps say
   "keep gesture-only preview outside the workspace collection" without saying
   what that preview is made of.
2. **The no-output representation.** With no placeholder, an output that has no
   workspaces needs an explicit representation, and sway's answer is that the
   case cannot arise: `output_enable` creates a workspace before the output is
   usable. Our code must either guarantee the same or model the empty case
   deliberately.

Neither is a contradiction of the model above. Both need to be settled before or
during the next attempt rather than discovered inside it.

### Fifth attempt: the canary holds, and the remaining work is enumerated

The second implementation pass also reset rather than land a partial model, but
it moved the problem further than any previous attempt and produced the first
positive evidence for the round-two design.

**The canary can hold.** A partial conversion kept `117-workspace.t` at 50
reached and 50 passed after stopping window insertion from creating a
placeholder. Every earlier attempt broke `117` at assertions 2, 11, 15, 33 or
41. The design's central prediction therefore survives its first contact with
the code, which is the opposite of what happened to the three-state model.

**Focused partial measurements** against the gap-only set:

| File | Before | Partial |
| --- | ---: | ---: |
| `139-ws-numbers.t` | 3 pass, 5 fail | 8 pass, 0 fail |
| `503-workspace.t` | 2 pass, 16 fail | 11 pass, 7 fail |
| `514-ipc-workspace-multi-monitor.t` | 2 pass, 1 fail | 3 pass, 0 fail |
| `519-mouse-warping.t` | 1 pass, 2 fail | 3 pass, 0 fail |
| `528-workspace-next-prev-reversed.t` | 16 pass, 22 fail | 16 pass, 22 fail |
| `535-workspace-next-prev.t` | 16 pass, 22 fail | 16 pass, 22 fail |
| `515-create-workspace.t` | 1 pass, 1 fail | 0 pass, 2 fail |

`506-focus-right.t` aborted before emitting TAP. Note `528` and `535` are
unchanged rather than regressed, and `515` is worse, so the partial is not
simply a smaller version of the finished model.

**The native suite is the real gate, not the conformance files.** The partial
started at **81 failing native layout tests**. Replacing the placeholder
invariants and naming detached workspaces brought that to **16**. Those 16
cluster in four areas, and they are the enumerated remaining work:

1. legacy index actions,
2. output reconnect focus,
3. configured-workspace creation at runtime,
4. transitions that temporarily drain a monitor of workspaces.

**What a complete attempt needs**, beyond the seven steps: a dedicated
monitor-owned preview render object, plus coordinated replacement of *every*
index and create-next transition. Retaining only the identity and ordering
subset repeats the earlier partial-land regressions.

This is incomplete evidence, not a contradiction of the design above.

### The prerequisite red test, verbatim

The sixth attempt wrote this and confirmed it fails at the right seam. It is not
committed, because a failing `#[test]` would make main red and `#[ignore]` is a
silent lie about coverage. Paste it into `src/layout/tests.rs` at the start of
the next attempt:

```rust
    #[test]
    fn sway_workspace_order_is_stored_after_creation_and_reparenting() {
        let mut layout = Layout::default();
        Op::AddOutput(1).apply(&mut layout);
        for (id, name) in ["10", "2", "alpha", "beta", "gamma"].into_iter().enumerate() {
            layout
                .activate_sway_workspace(crate::command::WorkspaceTarget::Name(name.into()))
                .unwrap();
            Op::AddWindow {
                params: TestWindowParams::new(id),
            }
            .apply(&mut layout);
        }

        Op::AddOutput(2).apply(&mut layout);
        Op::FocusOutput(2).apply(&mut layout);
        for (id, name) in ["8", "3", "delta", "epsilon"].into_iter().enumerate() {
            layout
                .activate_sway_workspace(crate::command::WorkspaceTarget::Name(name.into()))
                .unwrap();
            Op::AddWindow {
                params: TestWindowParams::new(id + 5),
            }
            .apply(&mut layout);
        }

        let output1 = layout.outputs().find(|o| o.name() == "output1").unwrap().clone();
        let output2 = layout.outputs().find(|o| o.name() == "output2").unwrap().clone();
        for name in ["2", "alpha"] {
            let (index, _) = layout.find_workspace_by_name(name).unwrap();
            layout.move_workspace_to_output_by_id(index, Some(output1.clone()), &output2);
        }

        let stored = layout
            .monitor_for_output(&output2)
            .unwrap()
            .workspaces
            .iter()
            .filter_map(Workspace::sway_name)
            .collect::<Vec<_>>();
        assert_eq!(stored, ["2", "3", "8", "delta", "epsilon", "alpha"]);
    }
```

It asserts the **expected** order only. Three attempts have now observed three
*different* stored orders for the same operations:

| Attempt | Observed |
| --- | --- |
| 4 | `8, delta, 3, epsilon, 2, alpha` |
| 5 | `epsilon, delta, 3, 8, 2, alpha` |
| 6 | `8, 3, delta, epsilon, alpha, 2` |

So the wrong order is not stable, and asserting it would produce a test that
passes for the wrong reason. The expected order is fixed by sway's comparator
and is the only safe assertion.

### Sixth attempt: independent confirmation, and one correction

The sixth attempt recovered attempt 4's patch, adapted it after the gesture
deletion, and reproduced attempt 5's four clusters independently. Findings that
change what the next attempt should do:

- Immediate inactive-empty cleanup restores `117-workspace.t` to 50/50. Attempt
  4 left it at 49/1 on assertion 2, so this is the missing transition, and it is
  now confirmed twice from different starting points.
- **Removing** window-insertion placeholder creation exposed 17 native layout
  failures; **retaining** it reduced that to 13-14. The failures include
  transitions that leave a monitor with no named workspace at all, which is
  exactly the invariant this design needs.
- Attempt 4's recovered patch cannot be landed regardless of its score: it
  implements the `Persistent` lifecycle state that `workspace_consider_destroy`
  disproved.

The invariant to add lives in `Monitor::verify_invariants`
(`src/layout/monitor.rs:2047`), not `TilingTree::check_invariants`, which cannot
see a monitor's workspaces. That function already asserts
`!self.workspaces.is_empty()`; the missing half is that at least one workspace
is named.

### Seventh attempt: the invariant measures the real coupling

The seventh attempt added the named-workspace invariant to
`Monitor::verify_invariants` **first**, before any other change, as the spec
recommended. That single assertion immediately failed **80 of 90** focused
native layout tests.

That number is the most useful measurement in this whole file. It proves the
current transition model violates "every live monitor contains at least one
named workspace" *broadly*, not in a few edge cases. The four failure clusters
enumerated after attempts 5 and 6 were not a list of stale tests to fix; they
were the visible surface of a production model dependency.

The attempt then built a minimal partial using next-free output identities,
creation and reparent sorting, identity materialisation on activation and window
insertion, and direct stored-order navigation. It made the red test green, kept
`117-workspace.t` at 50/50, and reduced the focused native failures from 80 to a
single stale index assertion. Conformance results:

| File | Baseline | Partial |
| --- | ---: | ---: |
| `139-ws-numbers.t` | 3 pass, 5 fail | 8 pass, 0 fail |
| `503-workspace.t` | 2 pass, 16 fail | 14 pass, 4 fail |
| `506-focus-right.t` | 25 pass, 6 fail | 31 pass, 0 fail |
| `514-ipc-workspace-multi-monitor.t` | 2 pass, 1 fail | 3 pass, 0 fail |
| `515-create-workspace.t` | 1 pass, 1 fail | 2 pass, 0 fail |
| `519-mouse-warping.t` | 1 pass, 2 fail | 3 pass, 0 fail |

Regression gates held or improved: `117` 50/50, `132` 160/0, `166` 93 pass/4
skip/9 fail, `504` 11/20, `543` 20/43.

**And it was still refused**, for two reasons that matter more than the scores.

First, `528-workspace-next-prev-reversed.t` and `535-workspace-next-prev.t`
**regressed from 16/22 to 8/30** under direct stored-order navigation. Reverting
navigation to its prior derivation returned them only to baseline, never better.
Three attempts have now hit this same pair from three different directions, so
relative navigation is not a consumer of the stored order in the way the
"Consumer collections" section assumes.

Second, the partial reached those green numbers **while still retaining and
promoting the internal placeholder, using index-derived identity, and retaining
configured workspaces through `persistent`** — violating settled steps 1, 2, 6
and 7. It is the same shape attempts 1 to 3 landed and had to revert: a green
subset built on the mechanism the design exists to remove.

### What an eighth attempt actually needs

Not more implementation effort. An explicit mapping, written down before any
code, for **every** index-, last- and placeholder-dependent site. A grep for
`workspaces.len()`, `workspaces.last()`, `workspaces[...]` and
`active_workspace_idx` across `src/layout/` returns **184 sites** today (37 in
`mod.rs`, 80 in `monitor.rs`, the rest spread). The seventh attempt counted 159
that are semantically load-bearing.

The specific unspecified case, named by two attempts independently, is a
**temporary monitor drain**: a transition that momentarily leaves a monitor with
no named workspace while moving the last one elsewhere. Sway cannot reach that
state because `output_evacuate` and `workspace_create` are ordered so it never
arises. Our operations are not, and until each of those sites has a stated
replacement the invariant cannot hold mid-transition even when it holds before
and after.

The complete per-function replacement map, corrected relative-navigation rule,
and drain-safe implementation order are in
[Workspace index and placeholder site mapping](2026-09-16-workspace-site-mapping.md).

## ext-workspace boundary

`ext-workspace-v1` now filters workspaces with the same current membership rule
as `GET_WORKSPACES`: it includes an active empty workspace, a nonempty
workspace, or a workspace with sway identity, and it excludes the trailing
empty creation placeholder. Both protocols also use
`Workspace::sway_display_name` for the current index fallback. This prevents a
panel from showing the placeholder and prevents the two public protocols from
deriving different names.

The fallback is still position-derived. Replacing it with stable identity would
repeat the failed identity-only experiment unless creation, retirement,
navigation, and ordering change together. The ext-workspace fix therefore does
not materialize identity or alter lifecycle.

### Spec defect found by the experiment

The "Prerequisite experiment" section says to move `alpha` then `2`, but states
an expected order in which the moved nonnumeric workspace follows the existing
nonnumeric names. Sway's append-plus-stable-sort produces the stated expected
order only when `2` moves before `alpha`. The move order in that section is
therefore wrong, not the expected order.
