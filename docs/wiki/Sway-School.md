# Sway School

*A tree-first tutorial for the i3/sway layout model, using swayward's default
key bindings (`Mod` = Super).*

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
| `Mod+Shift+Space` | Toggle floating |
| `Mod+Shift+Q` | Close the focused window |

The full list is in the [cheat sheet](#cheat) at the bottom.

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

This config has `focus_wrapping no`, so focus **stops at the edge**. Press `Mod+h` (or `Mod+←`) on the leftmost window and nothing happens — you're already as far left as you can go.

**Quick check** — Which statement about focus is true in this config?

1. Multiple windows can be focused if you hold `Shift`
2. Focus wraps around at the edges
3. Exactly one window is always focused; focus moves with `Mod+hjkl` (or arrows) and stops at edges
4. Focus is determined by mouse position only

<details><summary>Answer</summary>

**3.** Exactly one window is always focused; focus moves with `Mod+hjkl` (or arrows) and stops at edges

Single focus, keyboard-driven, no wrap. (Mouse *can* change focus too because `focus_follows_mouse yes` is set, but it's not the only way.)

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

Once you've established a direction in a slot (e.g. made a vertical column), new windows opened *inside that slot* keep going in that direction — **without** needing the bracket key again.

Proof: focus a window inside your vertical column, hit `Mod+Return`, and the new terminal stacks into the column rather than popping out sideways.

> The slot **knows what it is**. Swayward carries the arrangement forward until you change direction again with `[` or `]`.

This is the moment the mental model clicks for most people. Everything from here on is variations on this one idea.

**Quick check** — You built a 3-window column with `Mod+V`. Now you focus the middle window and press `Mod+Return` (no bracket key first). What happens?

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
swaymsg -t get_tree | grep -E '"(name|layout)"' | head -40
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

### The bracket-key twist

The bracket keys (`Mod+V` and `Mod+B`) don't just "set a direction." They do something sneakier:

> They **ensure the focused window's parent is of the requested type**, wrapping the focused window in a new container if it isn't.

So if you're focused on A inside `splith [ A B C ]` and hit `Mod+V`, swayward silently rewraps A:

Now opening a new terminal places it inside the new splitv (next sibling of A):

### Swayward is smart: brackets are no-ops when the parent already matches

If the focused window's parent is **already** the requested layout, the bracket key does **nothing**. No new wrapping, no nested-container tower.

Hit `Mod+V` ten times in a row inside a splitv — the tree is unchanged after the first one (which itself was a no-op if you were already in a splitv). This is why your tree never gets bloated from idle keypresses.

### The full table

| Current parent layout | You press | What happens |
| --- | --- | --- |
| splith | `Mod+B` | **no-op** (already horizontal) |
| splith | `Mod+V` | wrap focused window in a new splitv |
| splitv | `Mod+V` | **no-op** (already vertical) |
| splitv | `Mod+B` | wrap focused window in a new splith |
| tabbed | `Mod+V` | wrap focused window in a new splitv |
| tabbed | `Mod+B` | wrap focused window in a new splith |

**Quick check** — You're focused on a window inside a `splitv` column. You press `Mod+V` ten times in a row, then open a new terminal. What happens?

1. The new terminal is wrapped in 10 nested splitv containers
2. The new terminal appears to the right of the column
3. The new terminal appears below the focused window in the same column (the bracket presses were no-ops)
4. Swayward crashes

<details><summary>Answer</summary>

**3.** The new terminal appears below the focused window in the same column (the bracket presses were no-ops)

Brackets are no-ops when the parent layout already matches. The first `Mod+V` did nothing (parent was already splitv); the next nine likewise.

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
| `Mod+Shift+Q` | **Focus child** (zoom back in) |

### What you'll see

Focus a window normally — the border wraps just that window. Press `Mod+A`: the border **expands** to wrap the entire parent container, possibly multiple windows. That's swayward saying "focus is now on the container, not the window."

Press `Mod+A` again → expands further to the grandparent, and so on up to the workspace itself.

Press `Mod+Shift+Q` to walk back down. Swayward remembers which child you came from.

### Why this matters

When focus is on a container, **commands act on the whole container**:

- **Send a whole column to another workspace**: focus a column container, hit `Mod+Shift+3` — the entire subtree moves.
- **Tab a whole group**: focus a row, hit `Mod+W` — every direct child becomes a tab (toggles back to a split on a second press).
- **Wrap a whole subtree in a new split**: focus a row, hit `Mod+V` — the entire row becomes the top child of a new splitv.
- **Float a whole group**: focus a container, hit `Mod+Shift+Space` — the whole subtree floats together.

**Quick check** — You have a row of 5 windows, all siblings (no nesting). You focus one, press `Mod+A`, then press `Mod+Shift+Space` (toggle floating). What floats?

1. Just the originally focused window
2. Nothing — you can't float a container
3. All five windows, as a group (the row container is what's focused after `Mod+A`)
4. Swayward picks one at random

<details><summary>Answer</summary>

**3.** All five windows, as a group (the row container is what's focused after `Mod+A`)

`Mod+A` selected the row container. Toggling floating on a container floats the whole subtree.

</details>

## L10 · Mutating the tree (changing what's already there)

You can *build* trees (L6, L7) and *read* trees (L8). Now: how to *change* them after the fact.

### Changing a container's layout (the easy mutation)

Focus any window. The window's **parent container** is what these commands act on:

- `Mod+W` → **toggle** parent layout between tabbed and split
- `Mod+B` / `Mod+V` → ensure parent is splith / splitv (wrapping if needed; see L8.75)

### Changing the tree shape (the powerful mutation)

Combine `Mod+A` from L9 with bracket keys from L8.75 to **wrap whole subtrees**. Recipe to add a status-bar-style window across the bottom of an existing layout:

1. Focus any window in the layout.
2. `Mod+A` until the border wraps the entire workspace content (everything you want above the new bar).
3. `Mod+V` → wraps that whole subtree in a fresh splitv.
4. `Mod+Return` → new terminal appears below the entire layout.

Without the `Mod+A` step, the bracket key would only wrap one window. `Mod+A` is what makes mutations apply at the *right scope*.

### Tabs that hold containers

A subtle but mind-bending consequence of L8.75 + L10: when you tab a row that contains a column, you get tabs where **one of the tabs is the entire column**. Click that tab and the screen fills with the column rendered as a splitv. You can build trees of arbitrary nesting, mixing splits and tabs, for any layout you want.

**Quick check** — You have a row of 3 windows. You focus the middle one and press `Mod+W` (toggle to tabbed). What happens?

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

It's the inverse of `Mod+<direction>`: arrow keys *read* the tree, shift+arrows *rewrite* it.

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
swaymsg -t get_tree | grep -B 1 '"type":' | grep -E '"(name|type)"'
```

### Mouse: hold Mod and drag

- `Mod` + **left-click drag** anywhere on the window → move it
- `Mod` + **right-click drag** → resize it

Requires `Mod (swayward always uses the configured mod key)` in your config.

### Two stages per workspace

`Mod+hjkl` only navigates within the layer you're in (tiled or floating). To cross between them:

- `Mod+a` → `focus mode_toggle` (jump between tiled tree and floating layer)
- Mouse → just hover, since `focus_follows_mouse yes`
- `Mod+D` → the fuzzel launcher (swayward binds no window picker by default)

> Each workspace has **two stages**: tiled (back) and floating (front). `Mod+a` is the curtain between them.

### Un-float placement gotcha

When you `Mod+Shift+Space` a floater back to tiled, it's treated as a **brand new window**: inserted via the L8.75 rule as the next sibling of the most recently focused tile. Swayward does *not* remember where the window came from before floating.

To control placement: `Mod+a` into the tree, walk to the desired neighbour, `Mod+a` back to the floater, then `Mod+Shift+Space`.

### Keyboard resize — use `px`, not `ppt`

### Auto-floating rules

Use a `window-rule` to auto-float specific apps. Find the `app_id` with:

```
swaymsg -t get_tree | jq -r '.. | select(.type?=="con" or .type?=="floating_con") | "\(.app_id // .window_properties.class // "?")  ::  \(.name)"' | grep -i <app-name>
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
| `open-floating true`, then `sticky enable` | Always-visible overlay (PiP, etc.) |
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
| `Mod+Shift+1..6` | Has a workspace → no longer no-workspace |
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

This config already uses sticky for Picture-in-Picture videos:

```
window-rule {
    match title="^Picture in picture$"
    open-floating true
}
// swayward implements the `sticky` command but binds no key to it by
// default. Add one, or run: swaywardmsg sticky enable
```

Pop a YouTube video out into PiP — it floats AND sticks, so it follows you across workspaces. You've been using sticky daily without knowing the name.

### Sticky vs scratchpad

|  | Scratchpad | Sticky |
| --- | --- | --- |
| Visible by default? | No (hidden) | Yes (always) |
| Per-workspace? | No (no workspace) | No (every workspace) |
| Summon needed? | Yes (`Mod+Minus`) | No (just there) |
| Use case | On-demand utility | Always-visible overlay |

**Scratchpad = drawer.** Pull it out when needed. **Sticky = sticker.** Always on the window.

If you ever want a manual sticky toggle, add: `bindsym Mod+? sticky toggle` to a free key.

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

- A bracket key surprises you (extra wrapper changes what "the parent" is)
- Move-window has more layers to escape than expected
- A layout toggle changes scope you didn't intend

The fix in all cases is the same one tool: `Mod+A` to navigate to the right scope first.

## Cheat sheet

### Open and close

| Key | Action |
| --- | --- |
| `Mod+Return` | Open terminal |
| `Mod+b` | Open browser |
| *(unbound by default)* | Launch your editor |
| `Mod+D` | Run launcher (fuzzel) |
| `Mod+Shift+Q` | Close focused window |

### Focus & move

| `Mod+h/j/k/l` or `Mod+←/↓/↑/→` | Focus left/down/up/right |
| --- | --- |
| `Mod+Shift+h/j/k/l` or `Mod+Shift+arrows` | Move focused window |
| `Mod+A` | Focus parent container |
| `Mod+Shift+Q` | Focus child container |
| `Mod+a` | Toggle focus between tiled tree and floating layer |
| *(unbound by default)* | Window picker, e.g. a fuzzel script |

### Layout

| `Mod+B` | Next window opens to the right (splith) |
| --- | --- |
| `Mod+V` | Next window opens below (splitv) |
| `Mod+W` | Toggle tabbed ↔ split |
| `Mod+f` | Fullscreen toggle |
| `Mod+Shift+Space` | Floating toggle |

### Resize

| `Mod+r` | Cycle width preset (33% → 50% → 67%) |
| --- | --- |
| `Mod+=` / `Mod+-` | Grow / shrink width |
| `Mod+Shift+=` / `Mod+Shift+-` | Grow / shrink height |

### Workspaces

| `Mod+1..6` | Jump to workspace |
| --- | --- |
| `Mod+Shift+1..6` | Send focused window/container to workspace |
| `Mod+Page_Up/Down` | Prev / next workspace |
| `Mod+Ctrl+Page_Up/Down` | Move container and follow |
| *(unbound by default)* | `workspace back_and_forth` — toggle to last workspace |
| `Mod+Shift+u/i` | Move workspace to output left/right |

### Scratchpad

| `Mod+Shift+Minus` | Stash focused window into scratchpad |
| --- | --- |
| `Mod+Minus` | Summon / hide / cycle scratchpad windows |

### System

| `Mod+Shift+C` | Reload the config |
| --- | --- |
| `Mod+Shift+slash` | Hotkeys cheatsheet (fuzzel) |
| `Print` | Screenshot |
| `Ctrl+Alt+Delete` | Session menu |

### Mouse (with `Mod` held)

| `Mod` + left-drag floater | Move |
| --- | --- |
| `Mod` + right-drag floater | Resize (grab from a corner!) |

### Inspect the tree

```
# Compact view
swaymsg -t get_tree | grep -E '"(name|layout)"'

# Find an app's app_id
swaymsg -t get_tree | jq -r '.. | select(.type?=="con" or .type?=="floating_con") | "\(.app_id // .window_properties.class // "?")  ::  \(.name)"'

# Send commands directly (bypass keybinds)
swaymsg "resize grow width 100 px"
swaymsg "[app_id=firefox] focus"
```

*You've finished sway school. Now go build weird layouts. 🌳*
