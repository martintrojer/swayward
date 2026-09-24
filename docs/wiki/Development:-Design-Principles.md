swayward is maximalist about i3 and sway features and conservative about every
other change. A project this committed to nested containers is a little absurd,
and it knows. It has chosen to be absurd carefully. The nested container tree,
sway IPC compatibility, and session stability take priority over development
speed.

## Preserve the tree model

Layout operations act on a fully nested tree. Split, tabbed, and stacked
containers are first-class nodes rather than presentation modes attached to a
flat list. Operations preserve focus, remove empty containers, collapse
redundant containers, and keep sibling proportions normalized.

## Match sway IPC

Every successful `SWAYSOCK` reply follows sway's JSON schema. Unsupported
requests return a well-formed error rather than a private extension or a hung
connection. Compatibility is more valuable than a richer swayward-only
protocol because existing bars, shells, and scripts already speak sway IPC.

## Protect the live session

A compositor crash terminates every Wayland client in the session. Recoverable
hardware and user errors produce warnings. An `ERROR` log entry indicates a
swayward defect. Code reachable from a live session must not panic.

## Keep inherited subsystems stable

Input, outputs, rendering, screencasting, accessibility, and protocol support
come from niri. Changes to inherited code carry a permanent upstream merge
cost, so new behaviour belongs in new modules when possible.

## Make optional effects truly optional

Disabled visual effects must not add rendering work. Short-lived animations may
render extra frames, but persistent effects must preserve damage tracking and
direct scanout whenever their geometry permits it.

---

*This page replaces niri-specific layout principles. The unchanged engine
principles are adapted from the niri documentation.*
