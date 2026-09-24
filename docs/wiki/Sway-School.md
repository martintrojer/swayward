**Cult of the Tree**

*A practical introduction to the i3/sway container model. No oath. No robes.
One suspicious level of enthusiasm for nested rectangles.*

The tree is not a bag of shortcuts. It is a small idea with a long reach:
windows live inside containers, containers live inside other containers, and
commands act on whichever part of that structure has focus. Once that clicks,
the key bindings stop looking like a spell book someone dropped down the
stairs.

This tutorial uses swayward's shipped defaults. `Mod` is Super in a full
session and Alt in a nested window. The model also applies to i3 and sway; only
the keys and configuration examples here are swayward's.

## The induction classes

1. **[Windows and focus](Tree-School:-Windows-and-Focus.md)** — open windows,
   move focus, and discover the insertion rule before anyone says “node.”
2. **[Splits become a tree](Tree-School:-Splits-and-Containers.md)** — build a
   nested layout and finally name the thing we have been quietly constructing.
3. **[Focus the container](Tree-School:-Focus-the-Container.md)** — move focus
   above a window and operate on a whole branch. This is where the robes would
   appear if we had a costume budget.
4. **[Reshape the tree](Tree-School:-Reshape-the-Tree.md)** — change layouts and
   move windows through container boundaries.
5. **[Floating, scratchpad, and sticky windows](Tree-School:-Floating-and-Scratchpad.md)**
   — the useful exceptions that stand in front of the tree or briefly leave it.

Each class ends with the original quick checks. Do them. The tree is patient.
We have, in this repository, confidently treated `splith` as the opposite of
what it means.

## Keep the keys nearby

The tutorial introduces commands in context. For lookup, use the
[tree command reference](Tree-Command-Reference.md). It contains every shipped
binding used by the classes, plus the useful commands that are unbound by
default.

## What success looks like

At the end, you can look at a layout, sketch its tree, predict where the next
window opens, focus a parent container, move a window across a boundary, and
explain why a scratchpad window is not merely “minimized somewhere.” We now
also see the tree in screenshots that were never about the tree, and have
stopped looking for a cure.

You do not need to install swayward to keep the model. Take it to i3, sway, hy3,
or wherever nested rectangles are treated with the respect they have somehow
convinced us they deserve.

[Begin Induction I →](Tree-School:-Windows-and-Focus.md)
