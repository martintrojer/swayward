**Induction V: floating, scratchpad, and sticky windows**

The tree has a front stage. Floating containers stand there. Scratchpad
containers leave the workspace and wait in a drawer. Sticky containers follow
you around like a note you attached to every desk. None of this disproves the
tree; it merely confirms that even a cult needs a utility cupboard.

## Floating containers stand in front

> A floating window is a floating container with one child. A floating
> container can also hold a whole subtree, complete with splits, tabs, and
> focus history.

That's the whole concept. Everything else is consequences. The workspace has a
tiled tree at the back and a floating tree at the front. We did not escape the
tree. We built another stage for it.

<!-- CAPTURED tree-05-floating-stage
     WHAT: A tiled; B floating and centred above it
     KEYS: open A and B, floating enable B, move B to centre
     WHY: show a one-window floating tree in front of the tiled tree
     SIZE: nested output 1280x800 logical; captured at host scale 1.5
-->

| What the compositor draws | What the tree remembers |
| --- | --- |
| ![Floating window B centred above tiled window A](_assets/shots/tree-05-floating-stage-v3.png) | ![Window A in the tiled tree and B on a separate floating stage](_assets/shots/tree-05-floating-stage.svg) |


### Toggle and verify

- `Mod+Shift+Space` → toggle the focused window or container between tiled and floating (i3/sway default).

Verify it actually floated by looking for `"type": "floating_con"` in the tree dump:

```
swaywardmsg -t get_tree -p | grep -B 1 '"type":' | grep -E '"(name|type)"'
```

### Float a whole branch

1. Open A, B, and C.
2. Focus B, press `Mod+V`, and open D. B and D now share a vertical split.
3. Press `Mod+A` to focus that split.
4. Press `Mod+Shift+Space` to float the split and both windows together.

You can still use `Mod+hjkl` to move focus inside the floating group. With a
child focused, `Mod+W` switches the group to tabbed layout and `Mod+E` returns
it to a split layout. To send the whole group to another workspace, focus any
child and press
`Mod+Shift+<number>`. The same rule applies to `Mod+Shift+Minus`: the scratchpad
takes the complete group, not one leaf.

Press `Mod+Shift+Space` again to return the branch to the tiled tree. Swayward
inserts the branch as one container. Its internal split survives the trip.

The tree dump makes the nesting explicit. Here, the floating root has two
children:

```json
{
  "type": "floating_con",
  "layout": "splitv",
  "nodes": [
    { "type": "con", "app_id": "foot" },
    { "type": "con", "app_id": "foot" }
  ]
}
```

The two stages from Induction V are two trees, not a tree and a bag of
rectangles. The tree knows no bounds.

### Mouse: hold Mod and drag

- `Mod` + **left-click drag** anywhere on the window → move it
- `Mod` + **right-click drag** → resize it

Both gestures are built in. They use whichever key `input { mod-key }` names: Super on a TTY, Alt in a nested winit window.

### Two stages per workspace

`Mod+hjkl` only navigates within the layer you're in (tiled or floating). Swayward binds no key to `focus mode_toggle`. To cross between the layers, bind one. This example uses `Mod+Tab`, which is unbound in the shipped config:

```kdl
binds {
    Mod+Tab { command "focus mode_toggle"; }
}
```

`focus tiling` and `focus floating` are available too if you want one key per layer. With `focus-follows-mouse` uncommented in the `input` node, hovering also crosses the layers.

> Each workspace has **two trees**: tiled (back) and floating (front). `focus mode_toggle` is the curtain between them.

### Un-float placement gotcha

When you `Mod+Shift+Space` a floating root back to tiled, swayward inserts it
via the rule from Induction II. It becomes the next sibling of the most recently
focused tile. Swayward does not remember where the root was before it floated.

For a single window, the window is the root. For a floating group, the whole
branch is the root, so its internal structure stays intact. To control the new
placement, cross into the tiled tree with your `focus mode_toggle` bind, walk
to the desired neighbour, cross back to the floater, then press
`Mod+Shift+Space`.

### Keyboard resize

`Mod+R` enters the `resize` mode that the default config defines. Inside that mode, `h`, `j`, `k`, `l` and the arrow keys resize the focused window by 10 px a step, and `Return` or `Escape` returns to the default mode. Directional floating moves count pixels only: sway's parser ignores a trailing `ppt` on `move left 25 ppt` and moves 25 pixels, and swayward follows sway.

### Auto-floating rules

Use a `window-rule` to auto-float specific apps. Find the `app_id` with:

```
swaywardmsg -t get_tree | jq -r '.. | select(.type?=="con" or .type?=="floating_con") | "\(.app_id // .window_properties.class // "?")  ::  \(.name)"' | grep -i <app-name>
```

Then add a rule like:

```
window-rule {
    match app-id=r#"^pavucontrol$"#
    open-floating true
}
```

### The "summon and dismiss" pattern

For utility popups you summon, type into, and dismiss — audio mixers, calculators, password managers — use a richer rule that floats *and* sets a known size and position:

```
window-rule {
    match app-id=r#"^wiremix-float$"#
    open-floating true
    default-column-width { fixed 720; }
    default-window-height { fixed 480; }
}

window-rule {
    match app-id=r#"^org\.gnome\.Calculator$"#
    open-floating true
    default-column-width { fixed 480; }
    default-window-height { fixed 600; }
}
```

Every launch: float, resize to a known size, centre on the active output. The window appears in the same predictable spot every time, with no manual cleanup.

### The window-rule cookbook

| Pattern | Use case |
| --- | --- |
| `open-floating true` | Just float; swayward picks size and position |
| `open-floating true` plus `default-column-width` / `default-window-height` | Summon-and-dismiss popup at a fixed size |
| `open-floating true`, then run the `sticky enable` command | Always-visible overlay, such as PiP |
| `Mod+Shift+Minus` after it opens | Stash into the scratchpad drawer |

**Quick check** — You focus a vertical split containing B and D, then press
`Mod+Shift+Space`. What does the tree dump contain?

1. Two independent `floating_con` nodes
2. One `floating_con` whose `nodes` contain B and D
3. No change, because only windows can float
4. One floating B while D stays tiled

<details><summary>Answer</summary>

**2.** One `floating_con` whose `nodes` contain B and D

The floating root holds the original subtree. Its children do not become
independent floaters.

</details>

**Quick check** — A window is floating. You press `Mod+Shift+Space` to un-float it. Where in the tiled tree does it land?

1. Back exactly where it was before it floated
2. Always at the end of the workspace's top container
3. As the next sibling of the most recently focused tile (insertion rule)
4. Wherever the cursor is hovering

<details><summary>Answer</summary>

**3.** As the next sibling of the most recently focused tile (insertion rule)

Swayward treats it as a brand new window. It doesn't remember pre-float position.

</details>

## The scratchpad is a drawer

> Scratchpad is a hideout for floating utility windows that aren't bound to any workspace. Stash with one key, summon with another.

We needed a place for the windows that refuse the tree, and a drawer was more honest than another layout mode.

### Bindings

| Key | Action |
| --- | --- |
| `Mod+Shift+Minus` | `move scratchpad` — stash focused window (it disappears) |
| `Mod+Minus` | `scratchpad show` — summon/hide; cycles through stash |


<!-- CAPTURED tree-06-scratchpad-hidden
     WHAT: A visible; B stashed and absent from the workspace
     KEYS: from tree-05, move B to scratchpad
     WHY: the disappearance is the drawer closing
     SIZE: nested output 1280x800 logical; captured at host scale 1.5
-->

| What the compositor draws | What the tree remembers |
| --- | --- |
| ![Only tiled window A remains visible after B is stashed](_assets/shots/tree-06-scratchpad-hidden-v3.png) | ![Window B hidden in the scratchpad outside A's workspace](_assets/shots/tree-06-scratchpad-hidden.svg) |


<!-- CAPTURED tree-07-scratchpad-summoned-v3
     WHAT: B summoned onto workspace 2, whose tiled side is a splitv of C and D
     KEYS: from tree-06, workspace 2, open C, split v, open D, scratchpad show
     WHY: show that the drawer has no workspace of its own, on a workspace whose
          shape differs from the one B left
     NOTE: summoning B back onto workspace 1 restores the exact position scene V
          left, so that frame was byte-identical to tree-05 and could not
          distinguish a correct restore from a capture that never ran.
     SIZE: nested output 1280x800 logical; captured at host scale 1.5
-->

| What the compositor draws | What the tree remembers |
| --- | --- |
| ![Scratchpad window B summoned above a tiled column of C and D on workspace 2](_assets/shots/tree-07-scratchpad-summoned-v4.png) | ![Window B summoned from the scratchpad onto workspace 2's floating stage](_assets/shots/tree-07-scratchpad-summoned-v2.svg) |

B is summoned onto workspace 2 here, not back onto the workspace it left,
because the restore is exact: summoned onto workspace 1 it lands on the same
pixels as the floating shot above, and the two frames come out identical. The
size and position survive the drawer; the workspace is whichever one you are on.

### The model: floating roots with no workspace

- Scratchpad containers are **always floating**. Stashing a tiled window or subtree converts its root to floating.
- They have **no workspace** — they appear on whichever workspace you're on when you summon.
- Each root remembers its **size and position**. Set once, summons always restore.
- You can stash any number of roots. `Mod+Minus` cycles through them; it does not pop or consume them.

Cycling: `hidden → A → hidden → B → hidden → A → …`.

### Eviction

No dedicated "unstash" command. Both routes work because they violate the "floating + no workspace" property:

| Method | Why it evicts |
| --- | --- |
| `Mod+Shift+Space` on a summoned window | Stops floating → must have a workspace → joins current |
| `Mod+Shift+1` through `Mod+Shift+0` | Has a workspace → no longer no-workspace |
| Close the window | Window doesn't exist |

### The useful pattern: stash on launch

For a music player or chat app you always want one keystroke away:

```kdl
window-rule {
    match app-id=r#"^spotify$"#
    sway-for-window-command "move scratchpad"
}
```

Launch the app, then use `Mod+Minus` whenever you need it. The rule moves the
window to the scratchpad as soon as it opens, so it never occupies a tiled
workspace slot.

**Quick check** — You have two terminals stashed in the scratchpad. You press `Mod+Minus` three times. What's on screen at the end?

1. Both terminals visible
2. One terminal visible (the second one in the cycle: show A → hide A → show B)
3. Both terminals removed from scratchpad permanently
4. Nothing visible — the third press hid everything

<details><summary>Answer</summary>

**2.** One terminal visible (the second one in the cycle: show A → hide A → show B)

show A, hide A, show B. The third press is a fresh "summon" and brings up the next one in the cycle.

</details>

## Bonus · Sticky

> A sticky window stays visible on every workspace you switch to. Only applies to floaters.

This is the one arrangement in which a window follows you around, rather than
the reverse. We include it for balance.

The shipped config floats Firefox's Picture-in-Picture player. It does not make it sticky:

```kdl
window-rule {
    match app-id=r#"firefox$"# title="^Picture-in-Picture$"
    open-floating true
}
```

Pop a YouTube video out into PiP and it floats above the tiles on that workspace. To make it follow you across workspaces, focus it and run `swaywardmsg sticky enable`, or bind a key. Swayward implements the `sticky` command and binds no key to it.

### Sticky vs scratchpad

|  | Scratchpad | Sticky |
| --- | --- | --- |
| Visible by default? | No (hidden) | Yes (always) |
| Per-workspace? | No (no workspace) | No (every workspace) |
| Summon needed? | Yes (`Mod+Minus`) | No (just there) |
| Use case | On-demand utility | Always-visible overlay |

**Scratchpad = drawer.** Pull it out when needed. **Sticky = sticker.** Always on the window.

For a manual sticky toggle, add a bind on a free key. This example uses `Mod+Ctrl+S`, which is unbound in the shipped config:

```kdl
binds {
    Mod+Ctrl+S { command "sticky toggle"; }
}
```

---

[← Induction IV: reshape the tree](Tree-School:-Reshape-the-Tree.md) · [Tree command reference →](Tree-Command-Reference.md)
