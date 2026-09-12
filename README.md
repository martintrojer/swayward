# swayward

**An i3/sway-compatible Wayland compositor, built in Rust on smithay.**

> sway, gone its own way.

swayward speaks sway's IPC protocol, implements i3's fully nested container
tree, and runs on a modern memory-safe engine with a complete portal
implementation and the visual features sway won't take.

Status: **design phase.** No code yet. The design lives in
[`docs/specs/`](docs/specs/).

---

## Why

**The i3/sway model is the best window management model anyone has built, and it
deserves a compositor that isn't capped.**

Manual tiling with a fully nested tree — splitting, nesting, tabbing, moving
whole subtrees, marking windows and addressing them by criteria — is more
powerful and more precise than anything that came after it. The compositors
getting attention right now either discard that model or reimplement a shallow
imitation of it.

The problem was never the model. The problem is that every existing home for it
has somebody else's ceiling overhead:

| Project | Ceiling |
|---|---|
| **sway** | capped at i3's feature set by explicit policy — permanently, however good the idea |
| **SwayFX** | structurally a follower: a patchset over sway plus scenefx, inheriting sway's constraints and a rebase tax |
| **Hyprland** | has the features, not the model — and asks the whole ecosystem to write Hyprland-specific backends |
| **niri** | superb engine, deliberately rejects the tree |

swayward: all of that power, no ceiling, none of the drama.

### Modern, and still solid

swayward is memory-safe Rust on [smithay](https://github.com/Smithay/smithay).
That isn't a fashion statement. A compositor crash takes down every running
application in your session, which makes it exactly the kind of program where a
use-after-free costs you real work.

But modern must not mean unstable. **swayward inherits sway's temperament, not
its competitors'.** It moves slowly on purpose. The goal is the best window
manager, built in a modern way, without surrendering the boring reliability that
lets people daily-drive sway for years. Where velocity and solidity conflict,
solidity wins — that's an invariant, not a mood.

### Who it's not for

If you don't love the nested tree, swayward has nothing for you. niri is
excellent and scrollable tiling is a genuinely good idea. swayward is for people
who tried the alternatives and wanted their tree back.

---

## Why doing it this way is actually clever

swayward is a **fork of niri** that replaces the layout engine with i3's tree and
the IPC layer with sway's protocol. That sounds like an odd parentage for a sway
clone. It's the whole trick.

### 1. The hard 70% is already written, and it's the boring 70%

Writing a compositor from scratch means writing kernel mode-setting, multi-GPU
buffer handling, damage tracking, libinput plumbing, session management, and a
shader pipeline. None of that is why anyone wants a window manager, and all of it
takes years to get right.

niri has done it. Forking inherits roughly **57k of its 81k lines** — and it's
precisely the half that's miserable to write and invisible when it works:

- `src/backend/` — DRM, udev, libinput, winit, multi-GPU, session handling
- `src/render_helpers/` — blur, shadows, corner radius, offscreen passes, damage
- `src/protocols/` (3,772 lines) — the nine wlr protocols smithay doesn't ship
- `src/dbus/` — mutter ScreenCast/DisplayConfig, GNOME Screenshot, login1, a11y
- `src/tests/` (4,126 lines) + a GTK visual-test runner — headless harness,
  proptest, insta snapshots

What we write is the part we actually care about: the tree, the IPC, the config.

### 2. Two decisions delete most of sway

sway is ~54k lines of C. Two choices collapse it before a line is written:

- **We promise IPC compatibility, not config compatibility.** swayward is
  configured in KDL. sway's config parser, its config-only command handlers and
  much of its 77-file command directory simply aren't needed. Migrating users get
  a translator script — a one-time cost instead of a permanent one.
- **GPL-3.0-or-later**, matching niri. This is what makes niri's code *liftable*
  rather than *re-implementable*. Under MIT we'd be retyping several thousand
  lines of fiddly protocol code for zero user-visible gain.

### 3. The seam was already cut

This is the part that's genuinely lucky. niri's `Workspace` already composes two
interchangeable layout engines side by side:

```rust
pub struct Workspace<W: LayoutElement> {
    scrolling: ScrollingSpace<W>,   // ← we replace this
    floating: FloatingSpace<W>,     // ← i3's floating layer, near enough
    floating_is_active: FloatingActive,
    // …
}
```

So swayward drops an i3 container tree in where `ScrollingSpace` sits, and keeps
`Layout`, `Monitor`, `Tile` and `FloatingSpace` untouched. `Tile` matters most:
it's where corner radius, shadows, blur, resize animations and render snapshots
all live. **Not touching it is how swayward gets the entire SwayFX-parity feature
set for free** — and why "full eye candy" costs nothing here while it would have
cost months on a greenfield build.

The i3-ness lives *inside* a workspace. That's the smallest diff that produces
the largest change in behaviour.

### 4. Refusing to invent a protocol gets us the ecosystem for nothing

Because swayward speaks sway's IPC, the existing tooling works on day one.
Nothing to port, nobody to petition:

- **waybar**, **i3status**, **i3blocks**, **autotiling**, every `swaymsg` script
  you've ever written
- **[Quickshell](https://quickshell.org)** ships a first-class `Quickshell.I3`
  module, documented as "I3/Sway IPC integration". Its natively supported
  compositors are Hyprland, sway and i3 — **not niri**, whose users get by with
  generic protocols and third-party plugins. A Quickshell config written for sway
  cannot tell swayward apart from sway.
- **[DankMaterialShell](https://github.com/AvengeMedia/DankMaterialShell)** lists
  sway among the compositors it works best with.

Read that twice: **swayward gets better shell integration than the compositor it
forked from.** Hyprland asked the ecosystem to adopt it. niri is still in the
queue. swayward skips the queue by not inventing anything.

It also explains why IPC fidelity is our hardest invariant rather than a
nice-to-have. Every byte of schema drift costs an integration we'd otherwise have
had for free.

### 5. The two features that prove the ceiling was real

Not a theoretical argument. Two things sway structurally couldn't give us, both
inherited from niri on day one:

- **Portals that work.** sway relies on `wlr-screencopy` and
  `xdg-desktop-portal-wlr`: no window picker, no usable region picker, no restore
  tokens, recurring breakage with apps that expect a complete portal. niri
  implements the mutter/GNOME D-Bus APIs and rides `xdg-desktop-portal-gnome`.
  We keep `wlr-screencopy` too, so `grim`, `wl-screenrec` and OBS still work.
- **Rounded corners, blur, shadows, dimming — and far past that.** Exiled to a
  fork in sway's world; already in niri's render pipeline. swayward takes the
  whole inheritance rather than a SwayFX-sized subset: 13 independently
  configurable animations with easing *or* spring physics, user-programmable
  open/close/resize shader hooks, colour-space-aware gradients (Oklab/Oklch),
  focus rings, workspace shadows, tab indicators, and `ext-background-effect` so
  layer-shell clients like waybar get blur too. None of it is work we have to
  do — it lives in `render_helpers/` and `Tile`, which the design leaves
  untouched.

### The honest cost

One real downside, stated plainly: swayward takes on a **permanent merge
relationship with a fast-moving upstream**, and our changes land in `layout/`,
whose `mod.rs` is 5k lines and among niri's busiest files. Every niri release
means a merge, and the discipline that keeps this viable (new modules, no
gratuitous edits to inherited files, upstream what's generic) will feel like
pointless ceremony exactly when we're in a hurry.

We think that's a good trade for 57k lines of solved problems. We're not
pretending it's free.

---

## Invariants

Checkable claims, not aspirations. Each is machine-verified or a defect.

**I1 — IPC compatibility is a contract, not a best effort.**
Every reply on `SWAYSOCK` is byte-schema-identical to sway's, or a well-formed
`{"success":false,"error":"…"}`. Never a third thing, never a hang.
*Verified by* golden fixtures captured from real sway, replayed as snapshots.
First on the list because schema drift breaks waybar and Quickshell *silently*,
and silent breakage loses everyone at once.

**I2 — The tree is always well-formed.**
No empty containers survive an operation. The focus path is always valid and
terminates on a leaf or an empty workspace. Sibling percentages sum to 1.
Containers that should collapse, collapse.
*Verified by* proptest over randomized operation sequences.

**I3 — Never crash the session.**
A panic takes down every running application. Recover and log. `unwrap()` is a
claim that you've proven it can't fail.

**I4 — An `ERROR` in the log is a bug.**
Inherited from niri verbatim. Warnings are for user and hardware misbehaviour;
errors are for ours.

**I5 — Diff against upstream is a budget, not a prohibition.**
Prefer new modules, keep inherited names, upstream generic fixes. When editing an
inherited file genuinely is the right fix, do it — and log it in
`docs/DIVERGENCE.md`. Making the cost visible beats forbidding it and being
quietly ignored.

**I6 — No feature ships without the thing that proves it works.**
Layout ops get proptest coverage. IPC replies get a golden fixture. Visual
features get a visual-test case. The infrastructure is inherited and cheap, which
removes the excuse.

**I7 — Solid beats fast.**
swayward moves more slowly than niri on purpose. A feature that isn't ready
waits. Stability regressions outrank new capability, always.

---

## What swayward is not

The refusals define the project as much as the features. These are settled.

- **Not a plugin host.** No plugin ABI, no dynamic loading, no scripting runtime
  beyond sway's IPC. Hyprland's plugin ecosystem is a permanent maintenance sink
  and a stability hazard — exactly the trade I7 refuses. Extend swayward over
  IPC, or send a patch.
- **Not an IPC innovator.** We don't "improve" sway's protocol, bolt on a second
  richer one, or add extra fields because ours would be better. Compatibility
  *is* the product. Genuinely new capability with no sway equivalent goes in a
  clearly separate namespace, or it waits.
- **Not config-compatible with sway.** IPC compatibility plus a translator
  script. Parsing sway's grammar forever is a bad trade for a one-time migration.
- **Not a config-churn project.** After 1.0 the config format is stable. Breaking
  changes need a migration path and a real reason.
- **Not mandatory-anything.** Every effect, animation and flourish can be turned
  off. You should be able to configure swayward into something indistinguishable
  from sway.
- **Not a scrollable-tiling compositor.** niri exists and is better at it. No
  runtime layout-mode switching, no "niri mode".
- **Not bug-compatible with sway.** Matching sway's quirks is a bug-report
  standard we take seriously, not a goal we chase into absurdity.

---

## Scope

**In, for v1:**

- Fully nested i3 container tree — `split-h`, `split-v`, `tabbed`, `stacked`,
  implicit container creation and collapse
- Floating windows; the complete scratchpad, wired to foreign-toplevel minimize
  so docks and taskbars see it
- sway's global named/numbered workspace model, with output assignment
- Marks, criteria, `for_window`
- sway IPC: 13 message types, the `get_tree` schema, event subscriptions,
  `SWAYSOCK`
- KDL configuration, plus a sway→KDL translator
- Inherited from niri: all visual effects, all protocols, full portal/D-Bus
  support, XWayland via `xwayland-satellite`

**Out:**

- Config-file compatibility with sway or i3 (translator instead)
- `bar {}` / `get_bar_config` / launching swaybar — *noted as future work*;
  waybar never asks for it
- Scrollable tiling, and any runtime layout-mode switching
- niri's own IPC protocol
- In-process XWayland/xwm
- Our own xdg-desktop-portal backend

---

## Design documents

- [Foundation and compatibility contract](docs/specs/2026-09-12-swayward-foundation.md)
  — the full design: architecture, all twenty key decisions with rationale,
  testing strategy, milestones.

## Credits

swayward stands on other people's work, and says so:

- **[niri](https://github.com/YaLTeR/niri)** by Ivan Molodetskikh — the engine
  swayward is forked from. Born out of niri, not married to it; the debt is real
  and acknowledged.
- **[smithay](https://github.com/Smithay/smithay)** — the Wayland compositor
  toolkit underneath it all.
- **[sway](https://github.com/swaywm/sway)** and **[i3](https://i3wm.org)** — the
  model, the IPC protocol, and the standard of reliability we're aiming at.
- **[SwayFX](https://github.com/WillPower3309/swayfx)** — proof that people want
  these features, and a map of which ones.

## Licence

GPL-3.0-or-later.
