# Sway School

*A tree-first tutorial for the i3/sway layout model, using swayward's default
key bindings (`Mod` = Super on a TTY and Alt in a nested winit window).*

The i3/sway tree is the most expressive tiling model there is, and it is worth
learning properly. This tutorial teaches the model rather than the keys: once
you can see the tree, every binding follows from it.

Adapted for swayward from [Sway School](https://martintrojer.github.io/sway-school/),
which teaches the same model against stock sway. Everything here applies to i3
and sway too; only the key bindings and the configuration syntax are
swayward's.

## Introduction

Swayward is a tiling Wayland compositor that uses i3 and sway's layout model. The default reaction to it is "I don't get how layouts work." This tutorial fixes that — not by listing keys, but by teaching you the **tree model** swayward uses internally. Once you see the tree, every keystroke becomes obvious.

The lessons build on each other. Each one ends with a quiz that lets you check your understanding before moving on. *Don't skip the quizzes* — they're how you find out whether the model actually clicked.

### The keys you'll use most

| Key | What it does |
| --- | --- |
| `Mod+Return` | Open a terminal |
| `Mod+h/j/k/l` *or* `Mod+←/↓/↑/→` | Move focus left/down/up/right |
| `Mod+Shift+h` etc. *or* `Mod+Shift+arrows` | Move the focused window |
| `Mod+V` / `Mod+B` | Next window goes below / right |
| `Mod+A` | Focus the parent container |
| `Mod+Ctrl+A` | Focus the child container |
| `Mod+Shift+Space` | Toggle floating |
| `Mod+Shift+Q` | Close the focused window |

Every key in this tutorial comes from
[`resources/default-config.kdl`](../../resources/default-config.kdl). Keys that
swayward ships unbound are labelled *(unbound by default)* where they appear.
The full list is in the [cheat sheet](#cheat-sheet) at the bottom.

## L1 · A window is just a rectangle

Open one terminal: `Mod+Return`. It fills the screen, minus a small gap.

> One window takes all the space available to it.

That's the whole lesson. No tree, no containers — just one window filling its space.

**Quick check** — You open one terminal on an empty workspace. What do you expect?

1. A small window centred on screen
2. A window that fills the workspace (minus the configured gap)
3. Swayward asks where to place it
4. A floating window

<details><summary>Answer</summary>

**2.** A window that fills the workspace (minus the configured gap)

A single tiled window expands to fill its workspace. Floating only happens when configured ([L12](#l12)) or via auto-rules.

</details>

## L2 · A second window has to share the space

Open another terminal. Swayward picks a direction (default: side-by-side) and **shrinks the existing window** to make room.

> Swayward never overlaps tiled windows. Space is always divided, never stacked.

This is the foundational difference between a tiling WM and a traditional one. There's no z-order to fight with for tiled windows.

**Quick check** — With two terminals open side-by-side, you open a third. What happens?

1. Third window opens floating on top
2. Swayward refuses — "too many windows"
3. All three windows shrink to ~1/3 each, in a row
4. The third replaces one of the existing two

<details><summary>Answer</summary>

**3.** All three windows shrink to ~1/3 each, in a row

Tiles divide space. The arrangement (here: a row) is preserved; new windows just take their share.

</details>

## L3 · Swayward remembers the arrangement

Swayward didn't *happen* to put your three windows in a row. It **decided** "these go horizontally" and **remembers** that decision. Open a fourth terminal — it joins the row, four side-by-side.

> Swayward is keeping a note: "these windows are arranged horizontally." That note will eventually have a name: *container* (L8).

For now, just hold the idea: arrangements are remembered, not coincidental.

## L4 · Focus: swayward always knows which window is active

One window has a coloured border (blue in the Catppuccin theme). That's the **focused** window — the one keystrokes go to.

Move focus with either of these — they're bound identically:

- `Mod+h` / `j` / `k` / `l` (vim-style: left/down/up/right)
- `Mod+←` / `↓` / `↑` / `→` (arrow keys, exactly the same)

Swayward defaults to `focus-wrapping "yes"`, which is also sway's default, so focus **wraps at the edge**. Press `Mod+h` (or `Mod+←`) on the leftmost window of a row and focus lands on the rightmost one. To stop focus at the edge instead, set `focus-wrapping "no"` in the `layout` node.

**Quick check** — Which statement about focus is true with the shipped config?

1. Multiple windows can be focused if you hold `Shift`
2. Focus is determined by mouse position only
3. Exactly one window is always focused; focus moves with `Mod+hjkl` (or arrows) and wraps at edges
4. Focus never leaves the window you opened first

<details><summary>Answer</summary>

**3.** Exactly one window is always focused; focus moves with `Mod+hjkl` (or arrows) and wraps at edges

Single focus, keyboard-driven, wrapping. The mouse does *not* change focus with the shipped config: `focus-follows-mouse` is commented out in `resources/default-config.kdl`. Uncomment it in the `input` node to get sway's pointer-focus behaviour.

</details>

## L5 · New windows appear next to the focused one

The single most important rule:

> A new window opens **next to whichever window is currently focused**, in whatever arrangement that window lives in.

Proof: with three terminals A, B, C in a row, focus the leftmost (A) and open a new terminal. The new window appears **between A and B**, not at the end of the row. Focus is the anchor.

**Quick check** — Three terminals in a row, side-by-side. You focus the middle one and open a fourth terminal. Where does it appear?

1. To the far right of the row
2. Below the middle terminal (stacked)
3. Between the middle and right terminals
4. It replaces the middle terminal

<details><summary>Answer</summary>

**3.** Between the middle and right terminals

Next to the focused window, in the same arrangement (the row).

</details>

## L6 · You choose the direction *before* opening the next window

So far swayward has been picking the direction (horizontal). Now you take the wheel.

| Key | Whisper to sway |
| --- | --- |
| `Mod+B` | Next window opens to the **right** (horizontal) |
| `Mod+V` | Next window opens **below** (vertical) |

These keys do nothing visible by themselves. They just set up the direction for the *next* window you open.

Recipe to build a column: focus a window → `Mod+V` → `Mod+Return`. The new terminal appears below.

## L7 · Arrangements stick to their spot

Once you've established a direction in a slot, such as a vertical column, new windows opened *inside that slot* keep going in that direction. You do not press the split key again.

Proof: focus a window inside your vertical column, hit `Mod+Return`, and the new terminal stacks into the column rather than popping out sideways.

> The slot **knows what it is**. Swayward carries the arrangement forward until you change direction again with `Mod+B` or `Mod+V`.

This is the moment the mental model clicks for most people. Everything from here on is variations on this one idea.

**Quick check** — You built a 3-window column with `Mod+V`. Now you focus the middle window and press `Mod+Return`, with no split key first. What happens?

1. The new terminal opens to the right (horizontal default)
2. The new terminal joins the column, stacked between the focused window and the next one
3. Swayward asks for a direction
4. The new terminal floats

<details><summary>Answer</summary>

**2.** The new terminal joins the column, stacked between the focused window and the next one

The slot remembers it's vertical. New windows in that slot stay vertical until you say otherwise.

</details>

## L8 · Containers — the tree, finally named

That "slot that knows what it is" has a name: a **container**.

> A container = a remembered arrangement, with windows (or other containers) inside it.

Containers can hold:

- **Windows** (the things you actually see), or
- **Other containers** (which themselves hold windows or more containers).

That nesting *is* the tree. Russian dolls of remembered arrangements.

### See your tree

Run this in any terminal:

```
swaywardmsg -t get_tree -p | grep -E '"(name|layout)"' | head -40
```

Read the output as nested boxes:

| Field | Means |
| --- | --- |
| `"layout": "splith"` | horizontal container (children side-by-side) |
| `"layout": "splitv"` | vertical container (children stacked) |
| `"layout": "tabbed"` | tabbed container (children stacked as tabs) |
| `"layout": "none"` | a *leaf* — a real window, not a container |
| `"name": "..."` | a window's title (containers have `null` names) |

### Worked example

A workspace with one tile on the left and a vertical column of two tiles on the right looks like this in the dump:

```
"name": "4",                          ← workspace 4
  "layout": "splith",                 ← workspace is horizontal
      "layout": "none",
      "name": "π - dotfiles",         ← a window (left side)
      "layout": "splitv",             ← a vertical container (the column!)
          "layout": "none",
          "name": "Chrome",           ← window in column
          "layout": "none",
          "name": "swaymsg …",        ← window in column
```

As a picture:

**Quick check** — In the worked example above, how many *containers* are there (not counting leaf windows)?

1. 1 (just the column)
2. 2 (the workspace itself, plus the column)
3. 3 (workspace, row, column)
4. 0 — only windows are real

<details><summary>Answer</summary>

**2.** 2 (the workspace itself, plus the column)

Workspace (splith) and the column (splitv) are both containers. The three windows are leaves.

</details>

## L8.5 · The correct mental model for splith / splitv

This trips up almost everyone. The naming convention:

- `splith` = children laid out **h**orizontally (a side-by-side **row**)
- `splitv` = children laid out **v**ertically (a stacked **column**)

The name describes **how the children are arranged**, NOT the direction of the divider line between them.

### Why it feels backwards

Most people's first instinct is to picture the *divider*:

- "vertical split" → a vertical line down the middle → windows side-by-side
- "horizontal split" → a horizontal line across the middle → windows stacked

That's the CSS / Photoshop / "split a board in half" intuition. It's perfectly reasonable — it's just **the opposite** of sway's. Swayward names from the **result's** point of view: see a row → splith. See a column → splitv.

### Cheat table

| You want… | Key | Container becomes | Tree dump shows |
| --- | --- | --- | --- |
| Next window to the right | `Mod+B` | horizontal row | `"layout": "splith"` |
| Next window below | `Mod+V` | vertical column | `"layout": "splitv"` |

**Quick check** — You see `"layout": "splitv"` in a tree dump. What does that container look like on screen?

1. A horizontal row of windows side-by-side
2. A vertical column of windows stacked top-to-bottom
3. A tabbed group with one visible window
4. A single fullscreen window

<details><summary>Answer</summary>

**2.** A vertical column of windows stacked top-to-bottom

splitv = children arranged vertically = a column. The "v" describes the children's shape, not the divider.

</details>

## L8.75 · The insertion rule (the one rule to rule them all)

Everything you've learned so far is consequences of a single rule.

> When you open a new window, sway:
> 1. Looks at the **focused window**
> 2. Finds the **container that window lives in** (its parent)
> 3. Inserts the new window **into that container, right after the focused one**

The new window is always a **sibling** of the focused window — same parent, immediately after.

### The split keys do more than set a direction

`Mod+V` and `Mod+B` run `split v` and `split h`. Those commands do this:

> They wrap the focused window in a **new container** of the requested layout, unless the focused window is the only child of an existing `splith` or `splitv`. In that case they rewrite that parent's layout instead of adding a node.

So if you're focused on A inside `splith [ A B C ]` and hit `Mod+V`, swayward silently rewraps A:

Now opening a new terminal places it inside the new splitv (next sibling of A):

### Repeated presses do not grow a tower

A split key never stacks wrapper on wrapper. After the first press the focused window is an only child of a `splith` or `splitv`, so every later press only rewrites that parent's layout. Hit `Mod+V` ten times in a row and the tree after the tenth press matches the tree after the first. This is why your tree never gets bloated from idle keypresses.

What a split key does **not** do is skip the wrap when the layout already matches. Focused on A in `splith [ A B C ]`, `Mod+B` still wraps A in a fresh `splith`, because A has siblings. Sway wraps there too (`sway/sway/tree/container.c:1565-1621`).

### The full table

| Where the focused window sits | You press | What happens |
| --- | --- | --- |
| only child of a splith or splitv | `Mod+B` or `Mod+V` | rewrite that parent's layout, no new node |
| has siblings in a splith | `Mod+V` | wrap the focused window in a new splitv |
| has siblings in a splith | `Mod+B` | wrap the focused window in a new splith |
| has siblings in a splitv | `Mod+B` | wrap the focused window in a new splith |
| has siblings in a splitv | `Mod+V` | wrap the focused window in a new splitv |
| in a tabbed or stacked container | `Mod+B` or `Mod+V` | wrap the focused window in a new splith or splitv |

**Quick check** — You're focused on a window inside a `splitv` column. You press `Mod+V` ten times in a row, then open a new terminal. What happens?

1. The new terminal is wrapped in 10 nested splitv containers
2. The new terminal appears to the right of the column
3. The new terminal appears below the focused window in a splitv, because presses 2 to 10 changed nothing
4. Swayward crashes

<details><summary>Answer</summary>

**3.** The new terminal appears below the focused window in a splitv, because presses 2 to 10 changed nothing

The first `Mod+V` wrapped the focused window in a splitv. From then on that window was an only child, so each later press only rewrote the wrapper's layout.

</details>

**Quick check** — You're focused on window A. The tree is `splith [ A B C ]`. You press `Mod+V`, then open a new terminal called NEW. What does the tree look like?

1. `splith [ A NEW B C ]` — NEW just inserted after A in the row
2. `splitv [ splith[ A B C ] NEW ]` — the whole row got wrapped, NEW went below
3. `splith [ splitv[ A NEW ] B C ]` — A wrapped in a splitv, NEW joined below A
4. `splith [ A B C NEW ]` — NEW went to the end

<details><summary>Answer</summary>

**3.** `splith [ splitv[ A NEW ] B C ]` — A wrapped in a splitv, NEW joined below A

`Mod+V` wrapped A in a fresh splitv (parent was splith, didn't match). NEW then became A's next sibling in that splitv.

</details>

## L9 · Focus parent / focus child

Until now, **focus** has always meant "a window is highlighted." But the tree has more than just windows in it — it has containers too. And sometimes you want to operate on **a whole container** instead of one window inside it.

| Key | Action |
| --- | --- |
| `Mod+A` | **Focus parent** (zoom out one level up the tree) |
| `Mod+Ctrl+A` | **Focus child** (zoom back in) |

### What you'll see

Focus a window normally — the border wraps just that window. Press `Mod+A`: the border **expands** to wrap the entire parent container, possibly multiple windows. That's swayward saying "focus is now on the container, not the window."

Press `Mod+A` again → expands further to the grandparent, and so on up to the workspace itself.

Press `Mod+Ctrl+A` to walk back down. Swayward remembers which child you came from.

`Mod+Shift+Q` is not focus child. It runs `kill`, which closes the focused window.

### Why this matters

When focus is on a container, **commands act on the whole container**:

- **Send a whole column to another workspace**: focus a column container, hit `Mod+Shift+3` — the entire subtree moves.
- **Tab a whole group**: focus a row, hit `Mod+W` — every direct child becomes a tab. `Mod+W` runs `layout tabbed`, which sets the layout rather than toggling it. Use `Mod+E` (`layout toggle split`) to get back to a split.
- **Wrap a whole subtree in a new split**: focus a row, hit `Mod+V` — the entire row becomes the top child of a new splitv.

Floating is the exception. Sway can float a whole container, swayward cannot: `floating toggle` with a container focused floats nothing. See [floating split containers](../KNOWN_DEVIATIONS.md#floating-split-containers).

**Quick check** — You have a row of 5 windows, all siblings (no nesting). You focus one, press `Mod+A`, then press `Mod+Shift+Space` (toggle floating). What floats?

1. Just the originally focused window
2. Nothing, because swayward floats windows and not containers
3. All five windows, as a group
4. Swayward picks one at random

<details><summary>Answer</summary>

**2.** Nothing, because swayward floats windows and not containers

Sway would float the whole subtree. Swayward's floating space holds window tiles rather than tree nodes, so a container target does nothing. This is a recorded deviation, not a setting you can change.

</details>

## L10 · Mutating the tree (changing what's already there)

You can *build* trees (L6, L7) and *read* trees (L8). Now: how to *change* them after the fact.

### Changing a container's layout (the easy mutation)

Focus any window. The window's **parent container** is what these commands act on:

- `Mod+W` → set the parent layout to tabbed
- `Mod+S` → set the parent layout to stacking
- `Mod+E` → toggle the parent between splith and splitv, or back to the last split axis from tabbed or stacking
- `Mod+B` / `Mod+V` → split the focused window into a new splith / splitv (see L8.75)

### Changing the tree shape (the powerful mutation)

Combine `Mod+A` from L9 with the split keys from L8.75 to **wrap whole subtrees**. Recipe to add a status-bar-style window across the bottom of an existing layout:

1. Focus any window in the layout.
2. `Mod+A` until the border wraps the entire workspace content (everything you want above the new bar).
3. `Mod+V` → wraps that whole subtree in a fresh splitv.
4. `Mod+Return` → new terminal appears below the entire layout.

Without the `Mod+A` step, the split key would only wrap one window. `Mod+A` is what makes mutations apply at the *right scope*.

### Tabs that hold containers

A subtle but mind-bending consequence of L8.75 + L10: when you tab a row that contains a column, you get tabs where **one of the tabs is the entire column**. Click that tab and the screen fills with the column rendered as a splitv. You can build trees of arbitrary nesting, mixing splits and tabs, for any layout you want.

**Quick check** — You have a row of 3 windows. You focus the middle one and press `Mod+W` (`layout tabbed`). What happens?

1. Only the middle window becomes tabbed (a single-tab group)
2. The whole row collapses into 3 tabs (parent container layout changes from splith to tabbed)
3. The middle window jumps to a new workspace
4. Nothing — you must focus the parent first

<details><summary>Answer</summary>

**2.** The whole row collapses into 3 tabs (parent container layout changes from splith to tabbed)

Layout commands act on the focused window's parent container. Same windows in the same order, different rendering.

</details>

## L11 · Moving windows through the tree

You already know `Mod+Shift+hjkl` moves a window. Now we look at what it *actually does*.

> `Mod+Shift+<direction>` walks the focused window through the tree in that direction — swapping with siblings, escaping outward, or entering inward as needed.

It is the inverse of `Mod+<direction>`: `Mod` plus a direction *reads* the tree, `Mod+Shift` plus a direction *rewrites* it.

### The three cases

When you press `Mod+Shift+l` (move right), one of these happens:

#### Case A — swap with a sibling

If there's a sibling immediately to the right in the same parent, they swap places.

#### Case B — escape outward

If there's no sibling that way, but a parent further up has space, the window pops *out* of its container.

#### Case C — enter a sibling container

If the next sibling in that direction is itself a container, the window dives **into** it.

### Bonus case — escape from a perpendicular layout

If you're inside a splitv and press "right," swayward walks *up* the tree until it finds a parent that supports horizontal movement, then drops the window there.

Swayward never gets stuck — it just keeps escaping until the direction makes sense.

**Quick check** — Tree is `splith [ A B* splitv[ C D ] ]` with B focused. You press `Mod+Shift+l` (move right). What happens?

1. B swaps with the column (column moves left, B moves right)
2. B enters the column at the top: `splith [ A splitv[ B C D ] ]`
3. B and C swap inside the column
4. Nothing — B can't move that direction

<details><summary>Answer</summary>

**2.** B enters the column at the top: `splith [ A splitv[ B C D ] ]`

B's right neighbour is a container (the splitv), so this is Case C: enter the container.

</details>

## L12 · Floating windows

> A floating window **lives outside the tree**. It has no parent, no siblings, no layout. It's just a rectangle that sits on top of everything tiled.

That's the whole concept. Everything else is consequences.

### Toggle and verify

- `Mod+Shift+Space` → toggle the focused window between tiled and floating (i3/sway default).

Verify it actually floated by looking for `"type": "floating_con"` in the tree dump:

```
swaywardmsg -t get_tree -p | grep -B 1 '"type":' | grep -E '"(name|type)"'
```

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

> Each workspace has **two stages**: tiled (back) and floating (front). `focus mode_toggle` is the curtain between them.

### Un-float placement gotcha

When you `Mod+Shift+Space` a floater back to tiled, it's treated as a **brand new window**: inserted via the L8.75 rule as the next sibling of the most recently focused tile. Swayward does *not* remember where the window came from before floating.

To control placement, cross into the tree with your `focus mode_toggle` bind, walk to the desired neighbour, cross back to the floater, then press `Mod+Shift+Space`.

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

**Quick check** — A window is floating. You press `Mod+Shift+Space` to un-float it. Where in the tiled tree does it land?

1. Back exactly where it was before it floated
2. Always at the end of the workspace's top container
3. As the next sibling of the most recently focused tile (insertion rule)
4. Wherever the cursor is hovering

<details><summary>Answer</summary>

**3.** As the next sibling of the most recently focused tile (insertion rule)

Swayward treats it as a brand new window. It doesn't remember pre-float position.

</details>

## L13 · Scratchpad

> Scratchpad is a hideout for floating utility windows that aren't bound to any workspace. Stash with one key, summon with another.

### Bindings

| Key | Action |
| --- | --- |
| `Mod+Shift+Minus` | `move scratchpad` — stash focused window (it disappears) |
| `Mod+Minus` | `scratchpad show` — summon/hide; cycles through stash |

### The model: a flat list of floaters with no workspace

- Scratchpad windows are **always floating**. Stashing converts a tiled window to floating.
- They have **no workspace** — they appear on whichever workspace you're on when you summon.
- Each window remembers its **size and position**. Set once, summons always restore.
- You can stash any number of windows. `Mod+Minus` cycles through them; doesn't pop or consume.

Cycling: `hidden → A → hidden → B → hidden → A → …`.

### Eviction

No dedicated "unstash" command. Both routes work because they violate the "floating + no workspace" property:

| Method | Why it evicts |
| --- | --- |
| `Mod+Shift+Space` on a summoned window | Stops floating → must have a workspace → joins current |
| `Mod+Shift+1` through `Mod+Shift+0` | Has a workspace → no longer no-workspace |
| Close the window | Window doesn't exist |

### The killer pattern: auto-stash on launch

For a music player or chat app you always want one keystroke away:

```
// swayward has no open-into-scratchpad rule yet; stash it by hand with
// Mod+Shift+Minus after it opens.
```

Launch the app → it briefly flashes, then disappears into the scratchpad. `Mod+Minus` summons it whenever needed. **Never wastes a workspace slot.**

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

## Reading the matrix

Swayward shows you the tree in **tab titles** when a container has only one child but sits inside wrapper containers. The shorthand:

| Symbol | Meaning |
| --- | --- |
| `H[ … ]` | splith container |
| `V[ … ]` | splitv container |
| `T[ … ]` | tabbed container |
| `S[ … ]` | stacked container (like tabbed but vertical title list) |
| bare name | a leaf window |

So `H[T[Alacritty]]` = an H container, holding a T container, holding one Alacritty window. Three nodes nested; only one is visible.

### What to do about orphans

Usually nothing. They don't affect rendering, focus navigation, or resize. They only bite when:

- A split key surprises you (an extra wrapper changes what "the parent" is)
- Move-window has more layers to escape than expected
- A layout toggle changes scope you didn't intend

The fix in all cases is the same one tool: `Mod+A` to navigate to the right scope first.

## Cheat sheet

Every binding below is in
[`resources/default-config.kdl`](../../resources/default-config.kdl). Rows
marked *(unbound by default)* name a command swayward implements but ships no
key for. Bind those yourself.

### Open and close

| Key | Action |
| --- | --- |
| `Mod+Return` | Open a terminal (`exec foot`) |
| `Mod+D` | Run launcher (`exec fuzzel`) |
| `Mod+Shift+Q` | Close the focused window (`kill`) |
| *(unbound by default)* | Open a browser, an editor, or any other app: add an `exec` bind |

### Focus and move

| Key | Action |
| --- | --- |
| `Mod+h/j/k/l` or `Mod+←/↓/↑/→` | Focus left, down, up, right |
| `Mod+Shift+h/j/k/l` or `Mod+Shift+arrows` | Move the focused window |
| `Mod+A` | Focus the parent container |
| `Mod+Ctrl+A` | Focus the child container |
| *(unbound by default)* | `focus mode_toggle`, `focus tiling`, `focus floating` |

### Layout

| Key | Action |
| --- | --- |
| `Mod+B` | Split the focused window horizontally (`split h`) |
| `Mod+V` | Split the focused window vertically (`split v`) |
| `Mod+W` | Set the parent layout to tabbed |
| `Mod+S` | Set the parent layout to stacking |
| `Mod+E` | Toggle the parent layout between splith and splitv |
| `Mod+F` | Fullscreen toggle |
| `Mod+Shift+Space` | Floating toggle, on a single window only |

### Resize

| Key | Action |
| --- | --- |
| `Mod+R` | Enter the `resize` binding mode |
| `h/j/k/l` or arrows, in resize mode | Resize the focused window by 10 px |
| `Return` or `Escape`, in resize mode | Return to the default mode |

### Workspaces

| Key | Action |
| --- | --- |
| `Mod+1` through `Mod+0` | Jump to workspace 1 through 10 |
| `Mod+Shift+1` through `Mod+Shift+0` | Send the focused window or container to that workspace |
| *(unbound by default)* | `workspace back_and_forth`, `workspace next`, `workspace prev` |
| *(unbound by default)* | `move workspace to output left`, and the other output targets |

### Scratchpad

| Key | Action |
| --- | --- |
| `Mod+Shift+Minus` | Stash the focused window into the scratchpad |
| `Mod+Minus` | Summon, hide, and cycle scratchpad windows |

### System

| Key | Action |
| --- | --- |
| `Mod+Shift+C` | Reload the config |
| `Mod+Shift+E` | Quit, with the confirmation dialog |
| `Ctrl+Alt+Delete` | Quit, with the confirmation dialog |
| `Mod+Shift+Slash` | Show the hotkey overlay |
| `Mod+Escape` | Toggle the keyboard-shortcuts inhibitor |
| `Super+Alt+L` | Lock the screen (`swaylock`) |
| `Print` | Screenshot |
| `Ctrl+Print` | Screenshot the whole screen |
| `Alt+Print` | Screenshot the focused window |

Volume, microphone, playback, and brightness keys are bound to their `XF86`
equivalents and keep working while the session is locked.

### Mouse (with `Mod` held)

| Gesture | Action |
| --- | --- |
| `Mod` and left-drag | Move the window |
| `Mod` and right-drag | Resize the window. Grab from a corner |

### Inspect the tree

```
# Compact view
swaywardmsg -t get_tree -p | grep -E '"(name|layout)"'

# Find an app's app_id
swaywardmsg -t get_tree | jq -r '.. | select(.type?=="con" or .type?=="floating_con") | "\(.app_id // .window_properties.class // "?")  ::  \(.name)"'

# Send commands directly (bypass keybinds)
swaywardmsg "resize grow width 100 px"
swaywardmsg "[app_id=firefox] focus"
```

*You've finished sway school. Now go build weird layouts. 🌳*
