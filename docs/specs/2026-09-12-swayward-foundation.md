# swayward — Foundation and Compatibility Contract

Status: approved design, pending review
Date: 2026-09-12

## Why swayward

**The i3/sway model is the best window management model anyone has built, and it
deserves a compositor that isn't capped.**

Manual tiling with a fully nested container tree — splitting, nesting, tabbing,
moving subtrees, marking and addressing windows by criteria — is a more powerful
and more precise way to manage windows than anything that came after it. The
modern Wayland compositors that get the attention either discard that model
(scrollable tiling, dwindle, master-stack) or reimplement a shallow imitation of
it. swayward's position is blunt: the newer projects have not understood how good
manual tree manipulation actually is.

But every existing home for that model has somebody else's ceiling above it:

- **sway** caps its feature set at i3's by explicit policy. Anything i3 lacks is
  out of scope, permanently, however good the idea.
- **SwayFX** is structurally a follower: a patchset against sway plus scenefx,
  inheriting every one of sway's constraints along with a rebase tax.
- **Hyprland** has the features but not the model, and asks the entire ecosystem
  to write Hyprland-specific backends for it.
- **niri** has a superb engine and deliberately rejects the tree.

swayward: all of that power, with no ceiling, and without the sway drama.

### Modern, and still solid

The second half of the pitch is the implementation. swayward is memory-safe Rust
on smithay — no C, no manual lifetime management in a process whose crash takes
down every running application. That is not a fashion statement; a compositor is
exactly the kind of program where a use-after-free costs you your session.

But modern must not mean unstable. swayward inherits sway's temperament, not
niri's: **it moves more slowly than niri and prizes being rock solid.** The goal
is the best window manager, built in a modern way, without giving up the
reliability that makes people daily-drive sway for years. Where velocity and
solidity conflict, solidity wins.

### Born out of niri, not married to it

swayward starts as a fork of niri because niri's engine is excellent and its
code is liftable. That is a starting point and a debt, not an identity. We track
upstream while it is useful, contribute back where we can, and diverge without
hesitation where niri's choices don't serve the tree. The relationship is
practical; the project is not a niri variant.

### The ecosystem is already written

Because swayward speaks sway's IPC protocol, the existing i3/sway tooling works
on day one — nothing to port, nobody to petition:

- **waybar**, **i3status**, **i3blocks**, **autotiling**, **swaymsg** scripts
- **Quickshell** has a first-class `Quickshell.I3` module documented as
  "I3/Sway IPC integration". Quickshell's natively supported compositors are
  Hyprland, sway and i3 — *not* niri, whose users rely on generic protocols or
  third-party plugins. A Quickshell config written for sway cannot tell swayward
  apart from sway.
- **DankMaterialShell** lists sway among the compositors it works best with.

So swayward gets better shell integration than the compositor it forked from,
for free. Hyprland asked the ecosystem to adopt it; niri is still waiting in the
queue. swayward skips the queue by refusing to invent a protocol.

This is also why IPC fidelity is the project's hardest invariant rather than a
nice-to-have: every byte of schema drift costs an integration we would otherwise
have had for nothing.

### The two features that prove the ceiling is real

The ceiling argument is not theoretical. Two concrete things sway could not give
us, both of which swayward inherits from niri on day one:

1. **Screen sharing and portals.** sway relies on `wlr-screencopy` and
   `xdg-desktop-portal-wlr`: no window picker, no usable region picker, no
   restore tokens, recurring breakage with applications that expect a complete
   portal. niri implements the mutter/GNOME D-Bus APIs and rides
   `xdg-desktop-portal-gnome` instead.
2. **Visual features.** Rounded corners, blur, shadows, dimming — exiled to a
   fork in sway's world, already implemented in niri's render pipeline.

swayward closes the gap: **the i3/sway experience, uncapped, on a modern engine.**

### Who it is not for

If you don't love the nested tree, swayward has nothing for you. niri is
excellent and scrollable tiling is a genuinely good idea. swayward exists for
people who tried the alternatives and wanted their tree back.

## The approach

swayward is a **fork of niri** that replaces the layout engine with i3's nested
container tree and replaces the IPC layer with sway's protocol.

This is deliberately not a port of sway to Rust. Two decisions collapse most of
sway's 53k lines of C before any code is written:

- We promise **IPC compatibility, not config compatibility** (Q1). sway's
  config parser, its 7 config-only command handlers, and much of its 77-file
  command directory are not needed. swayward is configured in KDL.
- We licence under **GPL-3.0-or-later** (Q2), matching niri, which makes niri's
  hard-won code liftable rather than re-implementable.

### Why fork rather than build on smithay directly

niri's code is not consumable as a library. `niri-ipc` is published to crates.io
(8 versions, ~100k downloads) but carries niri's own protocol, which is useless
to us. The `niri` crate itself is published only as a 0.0.0 name placeholder,
and `niri-config` and `niri-visual-tests` are not published at all. Everything
we want lives inside a binary crate. Lifting means forking.

What that fork buys, measured against the alternative of greenfield-plus-anvil:

| Inherited from niri | Approx. lines | Why it matters |
|---|---|---|
| `src/backend/` | — | DRM, udev, libinput, winit, multi-GPU, session |
| `src/render_helpers/` | — | blur, shadows, corner radius, offscreen passes, xray, damage |
| `src/protocols/` | 3,772 | the nine wlr protocols smithay lacks |
| `src/dbus/` | — | mutter ScreenCast/DisplayConfig, GNOME Screenshot, login1, a11y |
| `src/tests/` + visual tests | 4,126 + crate | headless harness, proptest, insta, GTK visual runner |
| `src/window/`, `src/layer/`, `src/animation/`, `src/ui/` | — | mapped types, animations, overlays |

Roughly 57k of niri's 81k lines carry over, and it is the half that is hard to
get right: kernel-mode-setting, damage tracking, shaders, multi-GPU.

### The three replaced subsystems

**Layout** (`src/layout/`, 23k lines). `Workspace` already composes two
interchangeable engines — `scrolling: ScrollingSpace` and
`floating: FloatingSpace`, with `floating_is_active` switching between them.
That is the seam. We add a `TilingTree` engine in place of `ScrollingSpace` and
keep `Layout`, `Monitor`, `Tile` and `FloatingSpace` (Q7). `Tile` in particular
is where corner radius, shadow, blur, resize animation and render snapshots all
live; not touching it is how we get the entire eye-candy feature set for free.

**IPC** (`src/ipc/`, `niri-ipc`). Replaced by `swayward-ipc` carrying sway's 13
message types and JSON schemas, on one socket, exporting `SWAYSOCK` (Q8).

**Config** (`niri-config`). Fresh KDL schema for i3/sway semantics, reusing
niri's `knuffel` infrastructure and keeping its `input`, `output`, `animations`,
`window-rule` and `layer-rule` blocks verbatim (Q13).

## The zen of swayward

swayward is **maximalist about i3/sway features and conservative about
everything else.**

Two rules settle most future questions without further discussion:

1. **Never skimp on an i3/sway feature.** The nested container tree, the full
   scratchpad, marks, criteria, numbered workspaces — these are the point of the
   project, not a roadmap to be trimmed. Fully nested containers with implicit
   creation and collapse, tabbed and stacked layouts, and the complete scratchpad
   are v1 scope, not v2.
2. **Never take a diff against niri you do not need.** New behaviour goes in new
   modules behind traits. Inherited config blocks keep their inherited names.
   Every avoided line of diff is a cheaper merge for the life of the project.

Where these two rules conflict, rule 1 wins — see the workspace model decision.
Born out of niri, not married to it.

## Invariants

These are checkable claims, not aspirations. Each one is either machine-verified
or a defect when violated.

**I1 — IPC compatibility is a contract, not a best effort.**
Every reply on `SWAYSOCK` is either byte-schema-identical to sway's, or a
well-formed `{"success":false,"error":"…"}`. Never a third thing, never a hang.
*Verified by:* golden fixtures captured from real sway, replayed as snapshots.
*Why it's first:* schema drift breaks waybar and Quickshell silently, and silent
breakage loses the entire user base at once. This invariant is the project's
value proposition in executable form.

**I2 — The tree is always well-formed.**
No empty containers survive an operation. The focus path is always valid and
terminates on a leaf or an empty workspace. Sibling percentages sum to 1.
Containers that should collapse, collapse.
*Verified by:* proptest over randomized op sequences, via the `Op` enum.
*Why:* this is where sway itself still ships bugs, and the tree is our thesis.

**I3 — Never crash the session.**
A compositor panic takes down every running application. Recover and log.
`unwrap()` is a claim that you have proven it cannot fail.
*Verified by:* review, plus fuzzing pressure from I2.

**I4 — An `ERROR` in the log is a bug.**
Inherited from niri verbatim, because it is what makes the log usable. Warnings
are for user and hardware misbehaviour; errors are for ours.

**I5 — Diff against upstream is a budgeted resource, not a prohibition.**
Prefer new modules; keep inherited names; upstream generic fixes. When editing
an inherited file genuinely is the right fix, do it — and add a one-line entry to
`docs/DIVERGENCE.md` saying what and why.
*Verified by:* `git diff upstream/main --stat` staying legible, and every
inherited-file edit having a note. Making the cost visible beats forbidding it
and being quietly ignored.

**I6 — No feature ships without the thing that proves it works.**
Layout ops get proptest coverage. IPC replies get a golden fixture. Visual
features get a visual-test case. The infrastructure is inherited and cheap,
which removes the excuse.

**I7 — Solid beats fast.**
swayward moves more slowly than niri on purpose. A feature that is not ready
waits. Regressions in stability outrank new capability, always.

## What swayward is not

The refusals define the project as much as the features. These are settled, not
open for reconsideration at the first request:

- **Not a plugin host.** No plugin ABI, no dynamic loading, no scripting runtime
  beyond sway's IPC. Hyprland's plugin ecosystem is a permanent maintenance sink
  and a stability hazard — precisely the trade I7 refuses. Extend swayward over
  IPC, or upstream a patch.
- **Not an IPC innovator.** We do not "improve" sway's protocol, add a parallel
  richer protocol, or extend replies with extra fields because ours would be
  better. Compatibility is the product. New capability that genuinely has no sway
  equivalent goes in a clearly separate namespace, or it waits.
- **Not config-compatible with sway.** We promise IPC compatibility and ship a
  translator. Parsing sway's config grammar is a permanent tax for a one-time
  migration.
- **Not a config-churn project.** Once 1.0 lands, the config format is stable.
  Breaking changes need a migration path and a real reason.
- **Not mandatory-anything.** Every visual effect, animation and behavioural
  flourish can be turned off. Someone should be able to configure swayward into
  something indistinguishable from sway.
- **Not a scrollable-tiling compositor.** niri already exists and is better at
  it. No runtime layout-mode switching, no "niri mode".
- **Not bug-compatible with sway.** Matching sway's quirks is a bug-report
  standard we take seriously, not a design goal we chase into absurdity.

## Architecture

```
swayward/                 binary; forked niri core
  src/layout/
    tiling_tree/          NEW — i3 nested container tree
    workspace.rs          modified — composes TilingTree instead of ScrollingSpace
    monitor.rs            modified — sway workspace identity model
    tile.rs               UNCHANGED — corners, shadow, blur, animations
    floating.rs           mostly unchanged — i3's floating layer
  src/ipc/                REPLACED — sway protocol server
  src/protocols/          inherited verbatim
  src/dbus/               inherited verbatim
  src/render_helpers/     inherited verbatim
  src/backend/            inherited verbatim
  src/tests/              extended, never replaced
swayward-config/          forked niri-config; new layout + binds blocks
swayward-ipc/             NEW — sway JSON schema, published
swayward-visual-tests/    forked niri-visual-tests
contrib/sway-to-kdl       NEW — config translator script
```

### Data flow, layout

`Layout` → `Monitor` (per output) → `Workspace` → either `TilingTree` or
`FloatingSpace` → `Tile` → `Mapped` window. Only the `TilingTree` level is new.
`Tile` and below are untouched, which is what keeps rendering and animation
inherited.

`TilingTree` owns a tree of containers, each `split-h`, `split-v`, `tabbed` or
`stacked`, with leaves holding `Tile`s. It must satisfy the API surface
`ScrollingSpace` exposes to existing callers in `src/input/`, `src/ipc/`,
`src/ui/` and `src/layout/mod.rs` — 142 methods, most of them mechanical
forwarding.

### Data flow, IPC

Tree mutation → `Layout` marks dirty → IPC refresh pass (niri's existing
`ipc_refresh_*` pattern in `src/ipc/server.rs`) → diff against last sent state →
emit sway events to subscribed clients. Reuse niri's event-stream client
plumbing; replace only the payload types.

## Key decisions

| # | Decision | Rationale |
|---|---|---|
| Q1 | IPC byte-compatible; **config not compatible**, KDL instead | deletes sway's config parser and command tables; ship a translator instead |
| Q2 | GPL-3.0-or-later | makes niri's protocol and D-Bus code liftable |
| Q3 | fork niri | niri's internals are not published as libraries |
| Q4 | mutter/GNOME D-Bus **and** wlr-screencopy | portal-gnome for the real experience; screencopy for grim/OBS |
| Q5 | full SwayFX-parity effects | already implemented in niri; costs nothing |
| Q6 | git remote + periodic merge, upstream generic fixes | keeps the fork viable long-term |
| Q7 | `TilingTree` replaces `ScrollingSpace` inside `Workspace` | smallest diff; preserves `Tile` and all effects |
| Q8 | one socket, sway protocol, `swayward-ipc` published | one source of truth for window state |
| Q9 | inherit `xwayland-satellite` | free; no global X11 WM needed for this user |
| Q10 | first slice: `swaymsg -t get_tree` returns a valid i3 tree | forces tree model + IPC + schema into existence together |
| Q11 | ~83 runtime commands; real subset implemented, honest `{"success":false}` for the rest | a well-formed failure keeps clients alive |
| Q12 | golden fixtures captured from real sway **plus** proptest invariants | sway has no test suite; it cannot be our oracle |
| Q13 | fresh KDL for `layout`/`binds`; inherit `input`/`output`/`animations`/rules | minimises diff where niri has no i3 opinion |
| Q14 | no `bar {}` / `get_bar_config`; stub it | waybar never calls it; swaybar support is future work |
| Q15 | rename crates to `swayward*` in one mechanical commit, first | renaming later poisons every merge |
| Q16 | **fully nested tree, all of it, immediately** | this is the entire point of the project |
| Q17 | full sway scratchpad, wired to foreign-toplevel minimize | one feature, two audiences (sway users and taskbars) |
| Q18 | marks and full criteria | cheap, and heavily used in real configs |
| Q19 | niri's existing effect config blocks, names unchanged | zero diff for zero lost function |
| Q20 | **rebuild sway's global named/numbered workspace model** | bars key off `num`; this is the difference between sway-compatible and sway-flavoured |

### Q20 in detail, because it is the expensive one

sway workspaces are a global set, named or numbered, sparse, created on demand,
and assignable to outputs. niri's are per-output dynamic lists plus named ones.
Mapping niri's model onto sway's `get_workspaces` fields would leak visible
weirdness — `num` drifting from what bars expect, no `workspace N output X`, no
`workspace_layout` — into precisely the surface we promised was compatible.

So we take the larger diff: swayward implements sway's workspace identity,
numbering and output-assignment model, while keeping niri's `monitor.rs`
workspace-switch and animation machinery underneath it.

### Binds carry sway command strings

`binds { Mod+H { focus left; } }` — the action is a sway command string, parsed
by the same parser that serves `swaymsg`. One parser, one semantics, and users
can copy command syntax straight out of `man 5 sway`. Typed KDL actions were
considered and rejected: they would duplicate the command grammar.

## Testing approach

No new test infrastructure. Extend niri's, wholesale:

- **Headless harness** — `src/tests/fixture.rs` + `client.rs`: real compositor
  on a manually-dispatched `EventLoop`, driven by real `wayland-client` test
  clients. No TTY, no sleeps.
- **Randomized layout testing** — new tree operations go in the `Op` enum at the
  bottom of `src/layout/mod.rs`, which feeds proptest automatically. Gated by
  `RUN_SLOW_TESTS=1`, tuned with `PROPTEST_CASES`. Invariants to assert: no empty
  containers survive, the focus path is always valid, sibling percentages sum to
  1, implicit containers collapse when they should.
- **Golden IPC fixtures** — a script drives `swaymsg` against real sway across
  ~20 scenarios, capturing `get_tree`, `get_workspaces`, `get_outputs`. Committed
  as fixtures and replayed as insta snapshots against swayward.
- **Visual tests** — `swayward-visual-tests`, the forked GTK4 app, runs real
  layout and render code against mock windows. This is how corners, shadows,
  blur and animations get iterated without launching a session.
- **Nested dev loop** — winit backend, swayward in a window inside the current
  session. Then a TTY. Then daily-drive it.
- **Profiling** — Tracy via `profile-with-tracy-ondemand`, attachable to a live
  session.

Logging discipline is inherited and enforced: an `ERROR` in the log always
indicates a bug (I4).

**Ecosystem smoke tests.** Because the promise is that existing tooling works
unmodified, a fixed set of real clients is part of the definition of done:
waybar, a Quickshell config written for sway (via `Quickshell.I3`), `swaymsg`,
and `autotiling`. Each must run against swayward with no swayward-specific
changes. These are manual for now; automate what can be automated.

## Scope

### In scope for v1

- Fully nested i3 container tree: split-h, split-v, tabbed, stacked, implicit
  container creation and collapse
- Floating windows, full scratchpad (incl. foreign-toplevel minimize wiring)
- sway's global named/numbered workspace model with output assignment
- Marks, criteria, `for_window`
- sway IPC: 13 message types, `get_tree` schema, event subscriptions, `SWAYSOCK`
- Runtime command subset per Q11, with well-formed errors for the remainder
- KDL config; sway→KDL translator script
- Inherited: all effects, all protocols, all D-Bus/portal support, XWayland via
  satellite

### Explicitly out of scope

- Config-file compatibility with sway or i3 (translator script instead)
- `bar {}` config block, `get_bar_config`, `barconfig_update`, launching swaybar
  — **noted as future work**
- niri's scrollable-tiling layout, and any runtime layout-mode switching
- niri's own IPC protocol and `niri-ipc` compatibility
- In-process XWayland/xwm
- Behaviour-bug-compatibility with sway (a bug-report standard, not a goal)
- Our own xdg-desktop-portal backend

## Open questions

- Which ~20 scenarios go in the golden IPC fixture set? Decide when writing the
  capture script; needs a real sway to hand.
- Does the `TilingTree` API mirror `ScrollingSpace`'s 142 methods exactly, or do
  we introduce a narrower trait and adapt callers? Resolve while building
  milestone 1, when the real call sites are visible.
- Merge cadence with upstream niri: every release, or time-based?
- Does `docs/DIVERGENCE.md` (I5) track only inherited-file edits, or also
  deliberate behavioural deviations from sway? Leaning both, in two sections.

## Implementation checklist

Milestone 0 — fork hygiene
1. Fork niri at a tagged release; add upstream remote; record the base commit.
2. Mechanical rename: crates, binary, socket env var, config paths, docs.
3. CI green on the rename commit with no behaviour change.

Milestone 1 — the thesis slice (`swaymsg -t get_tree`)
4. `swayward-ipc` crate: sway's `get_tree` node schema, `get_workspaces`,
   `get_outputs`, `get_version`, command reply types.
5. IPC server on `SWAYSOCK`: socket, framing, message dispatch, event
   subscriptions.
6. `TilingTree` skeleton: container tree, one window, focus, geometry.
7. Wire `Workspace` to compose `TilingTree` in place of `ScrollingSpace`.
8. Golden fixture capture script; first snapshots green.

Milestone 2 — the tree, in full
9. split-h/split-v, implicit container creation and collapse.
10. tabbed and stacked layouts, reusing `tab_indicator.rs` rendering.
11. focus and move across the nested tree, in all directions.
12. Resize, sibling percentages, gaps, borders.
13. Tree ops added to the `Op` enum; proptest invariants written and passing.

Milestone 3 — sway semantics
14. Workspace identity model: global, named/numbered, sparse, output assignment.
15. Floating layer behaviour, scratchpad, foreign-toplevel minimize wiring.
16. Marks, criteria, `for_window`.
17. Command parser and the Q11 command subset; honest errors for the rest.

Milestone 4 — configuration and adoption
18. KDL `layout` and `binds` blocks; binds carry sway command strings.
19. `contrib/sway-to-kdl` translator, incl. SwayFX option names.
20. waybar running against swayward, unmodified.
21. Documentation: compatibility matrix, migration guide, known deviations.
