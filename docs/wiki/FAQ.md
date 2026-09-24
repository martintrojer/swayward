The first section answers the fair objections to swayward as a project. The
second is for people deciding whether to try it. The last is for people who
already tried it and would now like their titlebars back. A project with this
many nested containers was always going to need compartments in its FAQ.

## Why does swayward exist?

### Why not just use sway?

Often you should. [sway](https://github.com/swaywm/sway) is mature, and it is
the reliability standard swayward measures itself against. If sway already
does what you need, switching buys you nothing.

swayward is for people who want the same nested-tree model on
[niri's](https://github.com/YaLTeR/niri) Smithay foundation. That brings niri's rendering, effects, animations, overview, and
GNOME integration to a compositor whose layout and IPC follow i3 and sway.
Those are reasons to be curious, not evidence that sway made the wrong choices.

### Is this AI slop?

Fair question. swayward was built largely with AI assistance. That is stated
plainly because hiding it would be dishonest, not because it is a feature.

The answer is executable evidence: i3's test files are pinned in sway-ipc-oracle without edits
to their assertions, sway compatibility claims cite sway's C source, generated
tree operations search for broken invariants, and release packages must install
and run in clean distro containers. The Linux kernel's rule applies here too:
[AI does not sign off](https://www.kernel.org/doc/html/next/process/coding-assistants.html).
A human reviews the work and is responsible for it.

Do not trust us because we say the process was careful. Check the evidence.
“The agents seemed very confident” is not one of the checks.

### How can I trust a new compositor?

You should not trust one on introduction. A compositor crash takes the whole
session with it, so swayward tries to earn a test run before it asks for a daily
driver.

The suite includes:

- i3's upstream tests, pinned byte for byte in sway-ipc-oracle and run against a real headless
  compositor, real Wayland clients, and sway-shaped IPC;
- reply schemas and selected values captured from sway, plus row-by-row checks
  against sway's C source;
- randomized container-tree operations, with every found failure kept as a
  fast regression seed;
- clean-container installation tests for the Ubuntu tarball, `.deb`, Fedora
  RPM, and Arch package;
- CI checks for the supported Rust version, Clippy, formatting, feature
  combinations, the config translator, documentation links, and visual-test
  compilation.

This is not a comparison with anyone else's engineering. It is how a young
compositor pays for asking you to risk a session on it.

It is also not proof that swayward is ready for your machine. Hardware coverage
and time in live sessions remain thin. The suite is why we think the early beta
is worth testing, not why we think testing is finished. The current counts and
the limits of the oracle are explained in [Testing and
conformance](Testing-and-Conformance.md).

### Doesn't SwayFX already do this?

[SwayFX](https://github.com/WillPower3309/swayfx) keeps sway and changes its
rendering. That is a very good answer if you want effects on real sway.

swayward makes a different trade: it keeps the i3 model and sway IPC, but puts
them on niri's Smithay foundation. We did not set out to win a blur contest.
We kept the tree and replaced the foundation under it. `swayward-sway-to-kdl`
accepts SwayFX configuration too, so you can evaluate both without rewriting
everything by hand.

### Why not use hy3 on Hyprland?

Consider it. [hy3](https://github.com/outfoxxed/hy3) by outfoxxed is
the closest existing answer to the same desire: i3 and sway-style manual
tiling, node-based manipulation, and tabbed groups inside Hyprland. If you
already use Hyprland or want its plugin ecosystem, hy3 is the shorter path.

The difference is where the model lives. hy3 is a layout plugin inside
Hyprland. In swayward, the tree is the compositor's own model and sway's IPC
comes with it, so existing sway bars, scripts, and integrations can use the
same interface. A reader who picks hy3 still picked the tree. We count that as
a good outcome.

### Why fork niri?

Because niri had already solved a large collection of difficult compositor
problems well: rendering, input, output management, protocols, screencasting,
animations, and a Rust codebase built on Smithay.

swayward replaces the part where its goal differs: the scrolling layout becomes
an i3-style nested container tree, and the IPC surface becomes sway's protocol.
The fork exists because that combination is the project. A layout engine is not
a patch you mail upstream and hope it lands beside the scrolling strip.

### Why not contribute the tree work to sway?

The work replaces sway's foundation rather than extending it. The layout engine
and IPC behaviour are being implemented in a different Rust and Smithay
codebase. Little of that change would apply cleanly to sway's C and wlroots
implementation.

Sway remains the behavioural reference. A fix that belongs in sway should go to
sway; a second compositor architecture does not.

### Is swayward trying to replace sway or niri?

No. It serves the overlap between two preferences: people who want i3's tree
and sway's tools, but also want niri's modern foundation and additive visual
features.

If you prefer sway's maturity, use sway. If you prefer niri's scrolling strip,
use niri. swayward is not a successor to either one. It is the suspiciously
specific answer for people who looked at both and still wanted their tree back.

### Why is there no scrolling layout?

Because the tree is the point. We looked for a second point and did not find
one. Maintaining two layout models would make every
focus, move, workspace, and IPC operation carry two meanings.

niri's scrolling layout is excellent. swayward deliberately replaces it rather
than offering it as a mode. If the strip suits you better, use niri; you will
get the original idea from the people building it on purpose.

### Why use KDL instead of sway's configuration format?

The inherited niri subsystems already use typed KDL for outputs, input,
animations, effects, window rules, and portal integration. Rebuilding all of
that configuration in sway's language would create a second parser and a
second source of truth.

Swayward therefore keeps KDL and ships `swayward-sway-to-kdl` to migrate sway,
i3, and SwayFX configurations. The translator refuses directives it cannot
preserve instead of quietly dropping them. See [Migrate a sway config](https://github.com/martintrojer/swayward/wiki/SWAY_CONFIG_MIGRATION).

### How can swayward claim sway compatibility when its config is different?

The claim is IPC compatibility, not config-file compatibility. `SWAYSOCK`, the
binary protocol, JSON replies, events, and runtime commands are the interface
used by bars, scripts, and client libraries. Those either behave as sway does
or return a structured failure.

The KDL file is swayward's own configuration interface. The exact implemented
boundary is explained in [Sway compatibility](Sway-Compatibility.md), with
intentional differences in [Differences from sway](Differences-from-Sway.md).

### Why does swayward sometimes look fast and sometimes slow?

There are two clocks. Code can move quickly. Promises move slowly.

A handler landing does not make a sway command supported. A package file being
built does not make it installable. Both need executable evidence, and their
known limits need writing down before the compatibility table or release page
changes. That caution can make the public surface advance more slowly than the
commit history suggests.

Comparing commit rates with sway, niri, or Hyprland would explain little. They
have different scope, age, and release practices. The useful rule here is
simpler: stability first, protocol truth second, new features third. A live path
that panics skips the queue, overturns the desk, and becomes everybody's
immediate problem.

## Should I try it?

### Is swayward ready for daily use?

It is an early beta. It runs real sessions, but hardware coverage and outside
use are still thin. Try it if you know how to recover from a broken compositor
session and can report what happened.

Keep another compositor or desktop installed. Do not make swayward the only way
into a machine you need today.

### How compatible is it with sway?

Enough of sway's IPC exists for real clients such as Waybar and `swaymsg`, but
the surface is incomplete. Unsupported requests and commands return explicit
failures rather than convincing-looking partial data.

Read [Sway compatibility](Sway-Compatibility.md) for the current boundary.
Read [Differences from sway](Differences-from-Sway.md) before migrating a
session.

### What does the i3 test count prove?

It proves that unchanged i3 assertions reached through the harness observed the
expected behaviour. The test adapter replaces X11 window creation with real
Wayland clients; it does not rewrite assertions to make swayward pass.

It does not prove a compatibility percentage, smooth animations, delayed-client
behaviour, or support for X11 structures that sway itself does not expose.
[Testing and conformance](Testing-and-Conformance.md) explains both the
measurements and their limits.

### Why are some i3 tests skipped or unreached?

Some assertions depend on i3-specific X11 structures, i3bar, the i3 config
parser, or tree nodes sway does not have. Those cannot honestly prove sway
compatibility.

Permanent skips are itemised with a reason and a citation. Failures and
unreached assertions remain work. The machine-readable source is
[`coverage.toml`](https://github.com/martintrojer/swayward/blob/main/tests/i3/coverage.toml).

### Can I try it without replacing my current compositor?

Yes. Build swayward, then run it from an existing Wayland session:

```sh
swayward
```

It opens as a nested compositor window. Use `contrib/dev-run.sh` when developing
swayward itself; that script caps memory and time and keeps its IPC socket away
from your live session. See [Getting started](./Getting-Started.md).

### Can I migrate my sway or i3 configuration?

Use the bundled translator:

```sh
swayward-sway-to-kdl ~/.config/sway/config >~/.config/swayward/config.kdl
swayward validate -c ~/.config/swayward/config.kdl
```

The translator prints source-located errors for directives it cannot preserve.
The [migration guide](https://github.com/martintrojer/swayward/wiki/SWAY_CONFIG_MIGRATION) explains the remaining manual
work.

### Do Waybar and sway tools work?

Swayward exports `SWAYSOCK` and implements sway's IPC protocol. `swaywardmsg`
ships with the compositor, and `swaymsg` works for implemented requests. Waybar
can use its normal sway modules without a swayward-specific backend.

Compatibility is not complete. If a tool depends on an unsupported request or
field, swayward returns a failure. See [IPC](./IPC.md).

### How does screen sharing work?

The default path uses `xdg-desktop-portal-gnome`, including its window and
monitor picker and swayward's dynamic cast target. The less integrated
`xdg-desktop-portal-wlr` path remains available for monitor capture.

Portal support requires a full swayward session and the relevant packages. See
[Screencasting](./Screencasting.md) and [Important software](./Important-Software.md).
Remote control and input injection are not implemented.

### Which distributions have packages?

The release workflow builds an Ubuntu 24.04 `.deb`, one RPM for each supported
Fedora release, and an Arch package, then installs each one in a clean
container before drafting the release. It also builds an Ubuntu 24.04 binary
tarball.

The repository also provides a Nix flake. Install the current source with
`nix profile install github:martintrojer/swayward`, or use the flake as a NixOS
input. `nix flake check` runs the full test suite, including the pinned i3
conformance oracle.

No public beta release exists yet. COPR and AUR publication are deliberately
waiting until the release-page packages have survived real installations. We
would prefer the first package review not double as the first installation
test, thrilling though that would be.

### Where should I report a difference from sway?

[Open an issue](https://github.com/martintrojer/swayward/issues) with the command
or request, the result, what sway does, and enough configuration to reproduce
it. A short failing example is better than a broad compatibility claim.

Check [Differences from sway](Differences-from-Sway.md) first. If the difference is
already there, extra evidence that it matters to a real workflow is still
useful.

## How do I fix common setup problems?

### How do I disable client-side decorations?

Enable `prefer-no-csd` at the top level of the config, then restart affected
applications. swayward asks clients to omit their decorations and marks tiled
windows as tiled.

### Why are transparent windows tinted? Why is the border/focus ring showing up through semitransparent windows?

By default, the focus ring and border are solid rectangles behind windows.
Enable `prefer-no-csd`, or set `draw-border-with-background false` in a window
rule. See [Window rules](./Configuration:-Window-Rules.md#draw-border-with-background).

### How do I round window corners?

```kdl
window-rule {
    geometry-corner-radius 12
    clip-to-geometry true
}
```

### How do I run X11 applications?

Install xwayland-satellite 0.7 or newer. swayward creates the X11 socket and
starts it on demand. See [Xwayland](./Xwayland.md).

### Why doesn't swayward integrate Xwayland directly?

xwayland-satellite contains the X11 window-manager integration and presents X11
clients as regular Wayland windows. Keeping that complexity out of swayward
reduces the compositor's maintenance and crash surface.

### How do I recover from a failed screen locker?

Start another locker on swayward's Wayland display from a different TTY. You
can also configure a locker bind with `allow-when-locked=true`. The red
background means that the session remains locked.

### How do I select output profiles?

Use [Kanshi](https://gitlab.freedesktop.org/emersion/kanshi) to apply output
configurations based on the connected monitors.
