# The swayward stance

What this project is, who it is for, and how it asks to be judged. Every
public surface derives from this page: the README, the wiki, release notes,
the launch posts, and any reply to criticism.

This is the project's position, written for the people building it. Public
pages restate it in their own words rather than copying it.

**This page owns what swayward says.** Facts, positions and settled answers
belong here. How the docs sound is in
[`CONTRIBUTING.md`](../../CONTRIBUTING.md#writing-documentation).

## What it is

swayward is an i3/sway-model Wayland compositor: a fully nested container
tree, sway's IPC protocol on `SWAYSOCK`, built on niri's modern Smithay
foundation.

It is a fork, not a competitor and not a successor. niri is excellent and
still developed; swayward exists because we wanted i3's tree on that
foundation, not because anything was wrong with what was already there.

## Who it is for

People who tried the alternatives and wanted their tree back.

And, said as plainly: **if you don't love the nested tree, swayward has
nothing for you.** niri is excellent and scrollable tiling is a genuinely
good idea. Naming the second audience is what makes the first claim
credible.

## What it is trying to spread

The tree is a genuinely powerful idea that most people have never had
explained to them, and swayward has a second goal beyond being a good
compositor: **teach it.**

Splits inside splits, tabbed and stacked containers as first-class
structure, marks and criteria as addressing, the scratchpad. Once someone
sees the tree, every keystroke follows from it. Most users of conventional
desktops have never been shown that such a model exists, and most tiling
newcomers meet a keybinding list rather than the idea underneath it.

So the project invests in teaching as a first-class output, not as
documentation overhead:
[Cult of the Tree](../wiki/Sway-School.md) teaches the model from first
principles, and
[Why care about window management](../wiki/Why-Window-Management.md)
argues the case for anyone who has never questioned overlapping windows.

**A reader who learns the tree and then picks i3, sway or something else
entirely is a success, not a lost user.** That is the whole posture: the
idea is worth more than our share of it, and a project that only wins when
you install it has nothing to say to the people most worth reaching.

This is the one place swayward evangelises. It is never at another
project's expense: the tree belongs to i3, and saying so is part of
teaching it honestly.

## What it refuses to trade

Three things, in this order:

1. **The tree.** Maximalist about i3/sway features, conservative about
   everything else. The nested container tree, the full scratchpad, marks,
   criteria, numbered workspaces — these are the point, not a roadmap to be
   trimmed.
2. **sway's protocol.** On the wire there is no partial compliance. A request
   behaves exactly as sway's does, or it is not implemented and says so.
   Never a sway-shaped reply carrying swayward-shaped content.
3. **The live session.** A compositor crash takes every client with it. Code
   reachable from a live session must not panic, and stability regressions
   outrank new features.

It moves more slowly than niri on purpose. A feature that is not ready does
not ship.

## How it asks to be judged

Not on trust. On evidence you can check without our help:

- **An oracle we cannot edit.** `tests/i3/t/*.t` is i3's own suite, vendored
  byte for byte. `contrib/coverage-report` reports which files pass in full,
  unmodified. A test we could edit to pass would not be evidence.
- **Deviations cited to source.** Every intentional difference from sway is
  written down with a citation into sway's C, and `coverage-report --check`
  fails when the ledger drifts from the measurement.
- **Numbers that come from a command.** Where this project states a figure,
  a tool produces it rather than a person retyping it.

The honest limits, stated before anyone finds them: live-session testing is
thin, no stranger has installed it yet, and the i3 census contains real
skips and unreached assertions.

### On being built with AI

swayward was largely written with AI assistance. It is not a feature and it
is not hidden.

The project follows the Linux kernel's rule: AI does not sign off. Only a
human can certify a contribution, and the human is responsible for reviewing
it, for licence compliance, and for the result
(`Documentation/process/coding-assistants.rst`).

Disclosure is project-level, not per file. Every file here was touched the
same way, so a per-file marker would either be meaningless or imply that
untagged files were hand-written. The blanket statement is the accurate one.

The argument is not "trust us because we were careful". It is **don't trust
us, check us** — and the three items above are how.

## What it owes

- **niri** for the foundation: input, outputs, rendering, screencasting,
  protocols. The debt is large and gladly acknowledged.
- **sway** for the protocol and for the reliability standard swayward
  measures itself against.
- **i3** for the model, and for the test suite that keeps swayward honest.

These are not disclaimers. Standing on three excellent projects is the
interesting part of the story.

## Settled answers

Ordered by how early a stranger asks them, not by how interesting they are
to us. The first three decide whether someone keeps reading; the last two
only come up once they already care.

Consistency matters more than cleverness. Deliver these plainly, without
defensiveness:

- *"Why not just use sway?"* — Often you should, and we will say so. sway is
  mature, it is the reliability standard swayward measures itself against,
  and if it does what you need then switching buys you nothing. swayward
  exists for people who want that model on niri's newer foundation and the
  things that come with it — the effects, animations and overview inherited
  from niri, and a Rust/Smithay codebase rather than a C/wlroots one. Those
  are reasons to be curious, not reasons sway is wrong.

  One concrete example, since "newer foundation" is vague. swayward
  inherits niri's GNOME and Mutter D-Bus interfaces —
  `org.gnome.Mutter.ScreenCast`, `Mutter.DisplayConfig`,
  `Shell.Introspect`, `Shell.Screenshot` — so it runs
  `xdg-desktop-portal-gnome` rather than needing the wlroots backend. That
  means the GNOME screencast picker with its window list, a dynamic cast
  target, and whatever the GNOME portal stack gains next, without swayward
  implementing each piece. `xdg-desktop-portal-wlr` still works for anyone
  who prefers it.

  State this as something swayward has, never as something sway lacks. sway
  1.12 added `ext-image-copy-capture-v1` and per-toplevel capture sources,
  so window sharing works there too.
- *"Is this AI slop?"* — Fair question. Here is the unmodified i3 suite, the
  deviation ledger citing sway's source, and a coverage check that fails when
  the ledger drifts. Don't trust us; check us.
- *"Doesn't SwayFX already do this?"* — SwayFX adds effects to sway and
  rebases on each sway release, which is a genuinely good answer if effects
  on real sway are what you want. It is also a different bet: SwayFX keeps
  sway and changes the rendering, swayward keeps the i3 model and changes
  the foundation underneath it. `contrib/sway-to-kdl` translates SwayFX
  configs as well as sway ones, because someone evaluating both should not
  have to rewrite their config to try the other.
- *"Why not just use hy3 on Hyprland?"* — Genuinely consider it; it is the
  closest thing to swayward that already exists. hy3 is a Hyprland plugin
  by outfoxxed, who also maintains Quickshell, and it implements i3 and
  sway-style manual tiling with node-based manipulation and tabbed groups.
  If you already run Hyprland, or you want Hyprland's effects and plugin
  ecosystem, hy3 gets you the tree without changing compositor.

  The difference is where the model lives. In hy3 the tree is a layout
  plugin inside a compositor built around a different default, so it rides
  Hyprland's plugin ABI and its scope is the layout. In swayward the tree
  is the compositor's own model, and sway's IPC comes with it, so existing
  sway bars, scripts and desktop integrations work unmodified. Neither is
  the better answer in general. If your config is a Hyprland config, hy3 is
  the shorter path; if it is a sway or i3 config, swayward is.

  Say this without a hint of rivalry. hy3 is evidence for our own argument
  — people want the tree badly enough to rebuild it inside a compositor
  designed around something else — and a reader who ends up on hy3 has
  still ended up with the tree, which is the outcome this project says it
  wants.

- *"Why so slow to add features?"* — Separate the pace of code from the pace
  of promises. The code can move quickly. swayward is deliberately slower to
  mark behaviour supported, call a package installable, or call a release
  ready. A feature is not done because a handler exists; it is done when its
  behaviour has executable evidence and its known limits are written down.
  Stability regressions still outrank new features. Do not turn this into a
  comparison of commit rates with sway, niri, Hyprland, or anyone else; those
  projects have different scope, age, and release practices, so the comparison
  explains little and ages badly.
- *"Will you add a scrolling mode?"* — No. niri is excellent and scrollable
  tiling is a genuinely good idea; it is just not what this is for.
- *"Why not contribute to sway instead?"* — Different foundation. The layout
  engine and IPC layer are replaced wholesale, so there is little here that
  would apply upstream.

## See also

- [`specs/2026-09-12-swayward-foundation.md`](specs/2026-09-12-swayward-foundation.md)
  — the full design of record this page compresses.
- [`CONTRIBUTING.md`](../../CONTRIBUTING.md#writing-documentation) — how to say it.
- [`../KNOWN_DEVIATIONS.md`](../KNOWN_DEVIATIONS.md) — the evidence.
