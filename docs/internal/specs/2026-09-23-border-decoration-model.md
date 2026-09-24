# The decorated box: border, titlebar and corner radius

Research only. No renderer change lands with this document. It exists because
five consecutive patches to the same junction (`e3cca93a`, `d22eab05`,
`82693b45`, the earlier corner work, and the held `dd14f07e`) each fixed one
case and exposed another. The maintainer's diagnosis is the frame:

> The border currently hugs the active window AREA and never the titlebar or
> the tab/stack header. That is wrong in general. The border must enclose the
> window area AND whatever header belongs to it, which also implies the border
> must be ROUNDED where the header is rounded.

This document verifies that diagnosis against sway, states the model swayward
should adopt, diffs it point by point against the four files that implement
the current one, invents and justifies the corner-radius invariant that sway
cannot supply, and records the `prefer-no-csd` decision.

## 0. Vocabulary

Two words carry all the weight, so they are fixed here.

**Decorated box.** The rectangle a container occupies in its parent's
coordinate space, *including* every decoration the container owns. In sway
this is `con->pending.{x,y,width,height}`.

**Content rect.** The sub-rectangle of the decorated box in which the client
buffer is placed. In sway this is `con->pending.content_*`.

A decoration is *interior* to a decorated box if the box contains it, and
*exterior* if a different box does. Every argument below reduces to asking
which box owns which edge.

## 1. What sway does, verified

The task note's sway grounding was re-read against
`/var/home/martintrojer/hacking/sway-1.12-build` line by line. It is correct.
The findings, restated in the vocabulary above, with the verification.

### 1.1 The titlebar is inside the decorated box

`container_set_geometry_from_content` (`sway/tree/container.c:1018-1039`):

```c
border_width = con->pending.border_thickness * (con->pending.border != B_NONE);
top = con->pending.border == B_NORMAL ? container_titlebar_height() : border_width;

con->pending.x      = con->pending.content_x - border_width;
con->pending.y      = con->pending.content_y - top;
con->pending.width  = con->pending.content_width + border_width * 2;
con->pending.height = top + con->pending.content_height + border_width;
```

The decorated box is content plus side and bottom borders plus, at the top,
either the titlebar or the border width. There is no third rectangle. **A
sway border does not "wrap" a window area, because no such rectangle is ever
materialised.**

### 1.2 The top border *is* the titlebar

`arrange_container` (`sway/desktop/transaction.c:392-460`) positions four rects
in the decorated box's own coordinates:

| rect | position | size |
| --- | --- | --- |
| `border.top` | `(0, 0)` | `(width, border_top)` |
| `border.bottom` | `(0, height - bb)` | `(width, border_bottom)` |
| `border.left` | `(0, border_top)` | `(border_left, height - bt - bb)` |
| `border.right` | `(width - br, border_top)` | `(border_right, height - bt - bb)` |

and `border_top` is resolved by style (`transaction.c:399-426`):

| style | `border_top` | who draws the top |
| --- | --- | --- |
| `B_NORMAL`, own titlebar | titlebar height, via `arrange_title_bar(con, 0, 0, width, border_top)` | this container |
| `B_NORMAL`, parent's titlebar | `0` | the parent (tab/stack strip) |
| `B_PIXEL` | `border_top ? border_width : 0` | this container |
| `B_NONE` | `0`, and `border_width = 0` | nobody |
| `B_CSD` | `0`, and `border_width = 0` | the client |

Two consequences matter and both are load-bearing below.

- **The left and right borders start at `y = border_top`, not at `y = 0`.**
  They begin *below* the titlebar. They never run alongside it.
- **In `B_NORMAL` the top border and the titlebar are the same rect.** Lines
  `402-407` enable `border.top` only when the container is *not* `B_NORMAL`;
  otherwise the titlebar node occupies that slot. Asking "does the border
  enclose the titlebar" is a category error in sway's model. The titlebar is
  the enclosure's top edge.

### 1.3 The titlebar carries its own border ring

`container_arrange_title_bar` (`sway/tree/container.c:352-368`):

```c
int thickness = config->titlebar_border_thickness;   /* default 1, sway/config.c:261 */
pixman_region32_init_rect(&background, thickness, thickness,
    width - thickness * 2, height - thickness * 2);
pixman_region32_init_rect(&border, 0, 0, width, height);
pixman_region32_subtract(&border, &border, &background);
```

The titlebar draws a full inset ring in `colors->border` around a background
in `colors->background` (`container_update`, `container.c:226-235`). The strip
is self-contained: it needs nothing from the container border to look
finished, on any of its four sides.

### 1.4 In a tab or stack the parent owns every child's titlebar

`arrange_children` (`transaction.c:289-352`), `L_TABBED` branch:

```c
arrange_title_bar(child, title_offset, -title_bar_height,
                  next_title_offset - title_offset, title_bar_height);
wlr_scene_node_set_enabled(&child->border.tree->node, activated);
wlr_scene_node_set_position(&child->scene_tree->node, 0, title_bar_height);
...
arrange_container(child, width, net_height, title_bar_height == 0, 0);
```

Four facts, each one an invariant swayward must reproduce:

1. The parent calls `arrange_title_bar` for **every** child, at a **negative**
   `y`, so the strip is drawn above the child's own decorated box.
2. `child->border.tree` is enabled **only for the active child**. Inactive tabs
   have a titlebar in the strip and no border at all.
3. The active child is recursed into with `title_bar = (title_bar_height == 0)`,
   i.e. normally `false`: *you do not draw your own titlebar, I already did*.
4. `gaps` is passed as `0` (`transaction.c:318`), matching
   `container_get_gaps` (`transaction.c:474-497`), which returns 0 if any
   ancestor is `L_TABBED` or `L_STACKED`.

`L_STACKED` is the same with `title_height = title_bar_height * count` and per-
child `y = i * title_bar_height`.

Layout agrees: `apply_tabbed_layout` / `apply_stacked_layout`
(`sway/tree/arrange.c:185-211`) give `parent_offset = 0` for a view and
`titlebar_height` (times child count, stacked) for a container, which is why
the render step needs the negative `y`.

### 1.5 The one thing sway does not have

`grep -rn "corner_radius\|radius" sway/tree/ sway/desktop/` returns nothing
relevant. **Sway has no corner radius.** Every rect above is axis-aligned and
square. Section 4 is therefore not a sway citation; it is an invariant this
project has to invent, and it is labelled as such.

## 2. The model swayward should adopt

Stated in sway's terms, with swayward's rounding added as a separate layer so
the two can be reasoned about independently.

### M1. One rectangle per container, decorations inside it

A container has exactly one geometric identity, its decorated box. The content
rect is derived by insetting, never the other way round:

```
top    = normal_titlebar ? titlebar_height : (border_top ? border_width : 0)
left   = border_left   ? border_width : 0
right  = border_right  ? border_width : 0
bottom = border_bottom ? border_width : 0

content = box.inset(left, right, top, bottom)
```

No code computes a "window area" and then grows a border outward from it.

### M2. Four border rects, positioned in box coordinates

Exactly the table in 1.2. In particular the left and right rects span
`[border_top, height - border_bottom)`, so **they do not run alongside the
titlebar**. This is the direct, mechanical answer to "the border must enclose
the titlebar": it does, by the top rect *being* the titlebar, not by the side
rects extending upward past it.

### M3. The top edge has exactly one owner

For any decorated box, `border_top` is drawn by precisely one of: the
container itself (`B_PIXEL`, or `B_NORMAL` with its own titlebar), its tab or
stack parent (`B_NORMAL` under a strip), or nobody (`B_NONE`, `B_CSD`). Never
two, never zero-by-accident. The current `decorated_by_parent` flag is this
idea already; the model keeps it and makes it total rather than advisory.

### M4. A titlebar is a complete decoration

A titlebar draws its own inset border ring on all four sides (1.3). It does
not borrow an edge from the container border and it does not leave one for the
container border to supply. This is the fact that dissolves most of the corner
bookkeeping: a self-contained decoration has no seam to negotiate.

### M5. The parent draws the strip; only the active child has a border

In `Tabbed`/`Stacked`, the parent emits one titlebar per child and enables the
border tree of the active child only. Inactive children contribute a title and
nothing else. Gaps are zero anywhere under a tab or stack ancestor.

### M6. Rounding is a property of the decorated box, not of the window

See section 4. Stated here for completeness: the radius belongs to the
outermost decoration of a decorated box, and is inherited downward as zero.

## 3. Point-by-point diff against the current implementation

Line numbers are at `c70b8003`.

### 3.1 `src/layout/tiling_tree/geometry.rs`

| # | Site | Today | Model | Verdict |
| --- | --- | --- | --- | --- |
| G1 | `assign`, leaf arm, `:176-192` | Computes `rect` (the box), then on titlebar insets `rect.loc.y += titlebar_height` and stores the result as `leaf_contents`. Only the *top* is inset here; the side and bottom borders are added later, inside `Tile`, by growing outward from the window size. | M1: the box is inset on all four sides in one place. | **Divergent, accidental.** The inset is split across two files and two directions, which is the structural cause of every corner patch. |
| G2 | `Geometry` struct `:12-18` | Exports `leaf_contents` (content-ish), `ipc_nodes` (box), `titlebars`, `titlebar_attached`, `border_edges`. The rectangle a *decoration* should be drawn in is not among them. | M1/M2: export the decorated box and let decorations be positioned inside it. | **Divergent, accidental.** `ipc_nodes` is already close to the decorated box but is treated as an IPC-only artefact rather than the authority. |
| G3 | `:169-171` `decorated_by_parent && fullscreen.is_empty()` | Sets `titlebar_attached`, which `Tile` reads *only* to zero two corners (see T2). | M3: the flag should mean "my top edge is drawn by my parent", and should drive `border_top = 0`. | **Divergent, accidental.** Right concept, wrong consumer. |
| G4 | `:176` `!decorated_by_parent && !fullscreen.contains(&id) && tile.has_sway_titlebar()` | Matches sway's `B_NORMAL && title_bar`. | M3. | **Agrees.** |
| G5 | `:232-241` `child_decorated_by_parent = decorated_by_parent && (SplitH \|\| index == 0)` | Propagates "parent draws my top" through splits. | Sway does not propagate: `arrange_children`'s `L_HORIZ`/`L_VERT` branches always pass `title_bar = true` (`transaction.c:355,370`). The strip is drawn once, by the tab parent, for the *container*, and the container's children each draw their own titlebars inside it. | **Divergent, deliberate in origin but wrong.** This exists to decide corner rounding, not top-edge ownership. Once the radius moves to the decorated box (section 4) the condition has no remaining consumer. Note it currently has **no visible effect in scenes THA/StHA**: those scenes render zero active-border pixels at all (see 5.3), so the branch is untested by the matrix. |
| G6 | `:262-311` tab/stack arm | Parent computes each child's title rect and recurses with `decorated_by_parent = true, suppress_gaps = true`. Content is offset by the full strip height. | M5. Matches `transaction.c:294-350` including the stacked `titlebar_height * count`. | **Agrees.** `suppress_gaps` is `container_get_gaps`'s behaviour reached by a different route, which is fine. |
| G7 | `:262-311` | Every child gets a titlebar; no child's border is suppressed. | M5: only the **active** child has a border. | **Divergent, accidental.** Inactive tabs are not visible so nothing renders today, but the geometry claims a border for boxes that must not have one, and the invariant is what later code will trust. |
| G8 | `:136-168` `border_edges` | Computes per-edge visibility for `hide_edge_borders` / `smart_borders`. | Matches `sway/tree/view.c:373-418`. | **Agrees.** |

### 3.2 `src/layout/tile.rs`

| # | Site | Today | Model | Verdict |
| --- | --- | --- | --- | --- |
| T1 | `tile_size` `:885-900`, `border_insets` `:825-839`, `window_loc` `:861-882` | The tile's size is the *window* size grown by the border insets. The border is derived from the window; the window is the primary. | M1: the box is primary and the content is derived by inset. | **Divergent, accidental.** This is the literal "border hugs the window area" the maintainer named. |
| T2 | `geometry_corner_radius` `:816-823` | `if self.titlebar_attached { radius.top_left = 0; radius.top_right = 0 }`. | M6: a container under a strip has zero top radius because the strip owns those corners; a container under a strip also has zero *top border*. The zeroing is right, the reason recorded is "a titlebar is attached", not "my top edge belongs to someone else". | **Divergent, accidental.** Correct output today, wrong justification, and it is the only consumer of `titlebar_attached`. |
| T3 | `update_render_elements` `:542-546` | `outer_radius = self.window.geometry_corner_radius()` — reads the **window**, bypassing T2's masking; then `radius = outer_radius.expanded_by(border_width)` feeds `self.border`. | M6. The border must use the *box* radius. | **Divergent and a live defect.** This is why the border keeps a rounded top corner where the titlebar has already squared it. See 5.2, which measures it. |
| T4 | `render` `:1170-1172` | `self.geometry_corner_radius()` — the **masked** value. | M6. | **Agrees**, and disagrees with T3 three hundred lines away in the same file, on the same frame. |
| T5 | `render` `:1381-1385` | Border rendered at `location + self.window_loc()`, sized from `border_window_size` = window size. | M2: border rects positioned in box coordinates, with left/right starting at `border_top`. | **Divergent, accidental.** Same root as T1. |
| T6 | `has_sway_titlebar` `:1659-1672` | `B_NORMAL` and not CSD. | 1.2's style table. | **Agrees.** |
| T7 | `update_border_config` `:1715-1727` | `config.off = csd \|\| style == None`; width from the style. | 1.2's style table: `B_NONE` and `B_CSD` both give `border_width = 0`. | **Agrees.** |

### 3.3 `src/layout/titlebar.rs`

| # | Site | Today | Model | Verdict |
| --- | --- | --- | --- | --- |
| B1 | `render_buffer` `:186-197` | Rounds only the top two corners; the bottom edge is drawn square "because it meets the window". | M4: a titlebar is a complete decoration. Under the model the bottom edge meets the *content*, and squareness there is correct — but for the reason that the box's bottom corners are elsewhere, not because the titlebar is a partial shape. | **Agrees on output, for a reason that must be restated.** |
| B2 | `render_buffer` `:176-184` | Fills a background. Draws **no border ring**. | M4 and `container.c:352-368`: sway draws a `titlebar_border_thickness` ring in the border colour. | **Divergent, accidental, and unimplemented.** swayward has no `titlebar_border_thickness`. Worth noting the gap; not worth fixing before the model lands. |
| B3 | `TitlebarRenderer::render` `:113-121` | Clamps radius to `min(width, height*2)/2`. | Sane. | **Agrees.** |
| B4 | `Titlebar` struct `:20-28` | Carries `rect`, `ipc_rect`, `state`, `visible`. Nothing about which box corners this strip segment owns. | M6 needs exactly one added fact: the pair of top corners this segment owns. This is what held commit `dd14f07e` adds as `top_corners: (bool, bool)`. | **Divergent; `dd14f07e` is the right shape of fix.** See section 6. |

### 3.4 `src/layout/tiling_tree/rendering.rs`

| # | Site | Today | Model | Verdict |
| --- | --- | --- | --- | --- |
| R1 | `render` `:269-272` | `tile.window().geometry_corner_radius()` — the **unmasked window** radius, for the titlebar. | M6: the strip is the outermost decoration, so it takes the **box** radius. | **Divergent by luck.** The value happens to be right because the strip *is* the outermost decoration; the expression is right by accident and the same expression at T3 is wrong. |
| R2 | `render` `:262-284` | One buffer per tab, each independently rounded. | M6 case TAB-N: interior seams must be square. | **Divergent, and the defect `dd14f07e` fixes.** |
| R3 | `update_render_elements` `:59-63` | `set_border_edges` then `set_titlebar_attached`, both derived from geometry. | M3. | **Agrees.** |
| R4 | `active_window_visual_rectangle` `:4-11` | `rect.loc += tile.window_loc(); rect.size = tile.window_size()` — the *window* rect, not the box. | M1. Consumers wanting "the focused thing" want the box. | **Divergent, accidental.** Low blast radius; listed for completeness. |

### 3.5 Summary of the divergence

Eleven divergences. Two are deliberate-in-origin (G5, B1) and both survive
only as scaffolding for corner rounding. The other nine are one structural
mistake seen from nine angles: **the window rect is treated as primary and the
decorations are grown outward from it**, whereas sway treats the decorated box
as primary and insets inward. T3-versus-T4 is the sharpest single symptom: the
same file computes the corner radius two ways on the same frame, and the
border gets the one that ignores the titlebar.

## 4. The corner-radius invariant

**Sway cannot arbitrate this.** It has no corner radius. What follows is a
swayward invariant, argued from first principles and checked against the
matrix. It is labelled as invented so a future reader does not mistake it for
inherited behaviour.

### 4.1 The invariant

> **Rounding belongs to the decorated box, not to the window.** The outermost
> decoration of a decorated box owns that box's four corners and is the only
> thing permitted to round them. Every edge interior to the box is square, and
> every corner of a nested box is square.

The maintainer's suggested wording was "the outermost decoration of a
decorated box owns the top corners; interior edges are square". **Argued for,
with one amendment:** say *four corners*, not *top corners*. Restricting to
the top leaves the bottom two corners unowned, and the bottom is where the
matrix actually shows the most damage — 16 of the 20 stray pixels in scene
`SA` are at the bottom (5.2). A rule that only covers the half that looked
broken is how this problem reached five patches.

Three reasons the invariant is right, none of them aesthetic:

1. **It makes ownership total.** Every corner of every box has exactly one
   owner, derivable from the tree without looking at siblings' pixels. A rule
   with an owner for every case cannot produce the seam notches and stray
   arcs that the per-case patches kept trading between them.
2. **It follows M1.** If the decorated box is the primary rectangle, its
   outline is the thing with a shape, and the window is an interior detail. A
   radius on the window is a radius on something that has no outline of its
   own.
3. **It is what sway would say if asked.** Sway's decorations are opaque rects
   that tile the box exactly, with no overlap and no gap (1.2, 1.3). Rounding
   is the only operation that can break that tiling, so restricting it to the
   box's outer boundary is the minimal extension that preserves sway's
   property.

### 4.2 Every case in the decoration matrix

`R` is the configured radius; `0` is square. "Outermost decoration" is read
top-down from the box's own top edge.

| # | Case | Matrix scene | Outermost decoration | Box top corners | Box bottom corners | Interior |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Single window, `B_NORMAL` | `SA` | titlebar | titlebar rounds `R` | border rounds `R` | window square |
| 2 | Single window, `B_PIXEL` | `SB` | border | border rounds `R` | border rounds `R` | window square |
| 3 | Single window, `B_NONE`/`B_CSD` | — | none | `0` (client's business) | `0` | — |
| 4 | Horizontal split, N children | `HA`, `HB` | each child is its own box | each child rounds its own `R` | each child rounds its own `R` | no shared edges; gaps separate them |
| 5 | Vertical split, N children | `VA`, `VB` | as case 4 | as case 4 | as case 4 | as case 4 |
| 6 | Tab strip, 1 tab | `TA`, `TB` | the strip (one segment, full width) | strip rounds both `R` | active child's border rounds both `R` | child top corners `0` |
| 7 | Tab strip, N tabs | `T2A`, `T2B` | the strip (N segments) | **first segment rounds top-left only; last rounds top-right only; all interior segment corners `0`** | active child's border rounds both `R` | seams square |
| 8 | Stack, 1 entry | `StA`, `StB` | the strip | strip rounds both `R` | active child's border | child top `0` |
| 9 | Stack, N entries | `St2A` | the strip (N rows) | **topmost row rounds both `R`; every lower row `0` on all four corners** | active child's border | row seams square |
| 10 | Split inside a tab | `THA`, `THB` | the tab strip | strip rounds both `R` | the **bottom-most** children of the split round their outer bottom corners; all others `0` | every interior child corner `0` |
| 11 | Split inside a stack | `StHA`, `StHB` | the stack strip | as case 10 | as case 10 | as case 10 |
| 12 | Tabs side by side in a split | `HTA`, `HTB` | each tab container is its own box | each strip rounds its own box's top corners independently | each active child's border | the two boxes share no edge |
| 13 | Stacks side by side | `HStA`, `HStB` | as case 12 | as case 12 | as case 12 | as case 12 |
| 14 | Stack nested in a tab | `TStA`, `TStB` | outer tab strip | outer strip rounds `R` | innermost active child's border | **inner stack strip is fully square** |
| 15 | Tab nested in a stack | `StTA`, `StTB` | outer stack strip | outer strip rounds `R` | innermost active child's border | **inner tab strip is fully square** |
| 16 | Fullscreen | — | none | `0` | `0` | already handled: `fullscreen` short-circuits in `geometry.rs:265-271` |

Cases 14 and 15 are where the invariant earns its keep. Today an inner strip
reads `tile.window().geometry_corner_radius()` (R1) and rounds itself
regardless of nesting, because nothing tells it that a decoration above it
already owns the box outline. Under the invariant "am I the outermost
decoration of my box" is a single tree query and the inner strip answers no.

Cases 10 and 11 are the ones the current `child_decorated_by_parent` heuristic
(G5) tries to approximate with `layout == SplitH || index == 0`. That
condition is about top corners only and has nothing to say about the bottom,
which is case 10's harder half.

### 4.3 Where the invariant is under-evidenced

Honest scope, per the audit rule. The matrix covers cases 1, 2, 4, 5, 6, 7, 8,
9, 10, 11, 12, 13, 14, 15. It does **not** cover:

- case 3 (`B_NONE` / `B_CSD`) — no scene sets those styles;
- case 16 (fullscreen) — no scene fullscreens;
- a stack with a split inside it *below the first row*;
- three or more levels of nesting.

Additionally, scenes `THA`, `StHA`, `TStA` and `StTA` render **zero** active-
border pixels (5.3), so the four most deeply nested cases in the table are
currently unverifiable by colour scan and were reasoned about from the tree
rather than measured. Any follow-up that claims to fix case 10, 11, 14 or 15
must first make those scenes produce an active border.

## 5. Measured evidence

All measurements are from the existing capture at
`/tmp/swayward-decoration-matrix/shots`, produced by
`./contrib/capture-decoration-matrix`. The active border colour is
`#ffc87f` = `(255,200,127)` (`resources/default-config.kdl:212`); the focused
titlebar is `rgb(71.4,117.3,163.2)` = `(71,117,163)` (`:236`).

### 5.1 Stray-pixel census, reproduced

Reproducing the task note's table, extended, and split by screen half. A pixel
is stray if it is border-coloured and has at most one border-coloured
4-neighbour.

| scene | square | rounded | rounded top half | rounded bottom half |
| --- | --- | --- | --- | --- |
| `SA` | 0 | 20 | 4 | 16 |
| `SB` | 0 | 32 | 16 | 16 |
| `HA` | 1 | 10 | 2 | 8 |
| `HB` | 1 | 16 | 8 | 8 |
| `TA` | 0 | 20 | 4 | 16 |
| `TB` | 0 | 20 | 4 | 16 |
| `T2A` | 0 | 20 | 4 | 16 |
| `T2B` | 0 | 20 | 4 | 16 |
| `StA` | 0 | 20 | 4 | 16 |
| `St2A` | 0 | 20 | 4 | 16 |
| `HTA` | 0 | 18 | 2 | 16 |
| `HTB` | 0 | 18 | 2 | 16 |
| `HStA` | 0 | 18 | 2 | 16 |
| `THA` | 0 | 0 | 0 | 0 |
| `TStA` | 0 | 0 | 0 | 0 |
| `StTA` | 0 | 0 | 0 | 0 |

The note's figures reproduce exactly. Two things it did not record:

- **The bottom half is the larger problem in every scene.** 16 of 20 in `SA`.
  This is what motivates widening the invariant from "top corners" to "four
  corners" (4.1).
- **`VA` is 0/20 and `SB` is 0/32**, both unlisted in the note. `SB` is
  `B_PIXEL`, i.e. no compositor titlebar at all, and it has the *worst* count
  in the matrix. Whatever is wrong is therefore **not caused by the titlebar**.
  The titlebar only hides four of the sixteen top-half strays by covering
  them.

### 5.2 The mechanism, localised to T3

Scene `rounded-SA`, top-left region, columns 10-60. `B` is active border, `T`
is focused titlebar, `#` is the shadow gradient:

```
y=16  .............####TTTTTTTTTTT     <- strip top edge, arc starts at x~27
y=20  ........#TTTTTTTTTTTTTTTTTTT
y=28  ......TTTTTTTTTTTTTTTTTTTTTT     <- arc closed, strip reaches x=16
y=56  ...............####BBBBBBBBB     <- strip bottom / border top, arc again
y=57  .............##BBBBBBBBBBBBB
y=58  .............#######........     <- NOTHING. no left border pixel
y=60  ..........###...............
y=64  .......##...................
y=65  ......#B#...................     <- left border resumes, 7px late
```

Row spans for the same image:

| y | titlebar span | border span |
| --- | --- | --- |
| 16 | 27..1886 | — |
| 28 | 16..1897 | — |
| 55 | 16..1897 | — |
| 56 | — | 29..1884 |
| 57 | — | 25..1888 |
| 58..64 | — | **none** |
| 70+ | — | 16..1897 |

The strip's arc closes at `y=28`, reaching the box's true left edge `x=16`.
Then at `y=56` the **border starts a second arc from scratch**, inset to
`x=29`, and the left border does not reach `x=16` until `y=69`. The border is
re-rounding a corner the titlebar already rounded, 40 pixels lower down. The
seven scanline gap at `y=58..64` is the visible result.

That is T3 exactly: `update_render_elements` at `tile.rs:542-544` reads
`self.window.geometry_corner_radius()`, which is the **unmasked** radius, so
the border rounds its top corners even though `set_titlebar_attached` has
already zeroed them for every other consumer. `render` at `:1170-1171` reads
the masked `self.geometry_corner_radius()`. The two disagree on the same
frame.

Same scene, square mode, for contrast: border spans `16..1897` from `y=56`
continuously. Zero strays.

### 5.3 Four scenes measure nothing

`rounded-THA`, `rounded-StHA`, `rounded-TStA`, `rounded-StTA` contain **zero**
pixels of `(255,200,127)`. A colour histogram shows they are rendering the
*inactive* border `(80,80,80)` throughout — 11875, 13916, 11555 and 11715
pixels respectively, with an active count of 0.

Their "0 strays" rows in the task note's table are therefore **vacuous**. They
are not evidence that nested layouts are clean; they are evidence that no
window is focused in those captures, so the scene being examined is not the
scene the harness intended. `HTA` by contrast has 8050 active and 5 inactive
pixels, so the harness *can* produce a focused nested scene; these four lose
focus somewhere in their command sequence.

This is a harness defect, and it must be fixed before cases 10, 11, 14 and 15
can be claimed. It is task `fix-decoration-matrix-nested-scenes-unfocused`
below.

### 5.4 The interior tab seam

`rounded-T2A` at `y=16`: unfocused segment `27..945`, focused segment
`968..1886`, a 22px gap of background at the seam. At `y=28` the same row
reads `16..956` and `957..1897` — flush. Two facing arcs eat a notch out of
the shared seam, closing by `y=28`. `square-T2A` is flush at every row.

This confirms the diagnosis in held commit `dd14f07e` independently.

## 6. Does corner radius stay a per-window rule?

**No. It should move to the decorated box.** The per-window rule is a niri
concept: in niri a window *is* the decorated thing, so a per-window radius has
a referent. In swayward the decorated thing is the container box, and a window
under a tab strip has no outline of its own to round.

Concretely:

- `geometry-corner-radius` stays a **window rule**, because that is its
  configuration surface and users have it in their configs. Removing it is a
  gratuitous break and section 4 does not require it.
- Its **meaning** changes: it is the radius *requested* for the decorated box
  that the window is the content of. The box resolves the request; the window
  does not apply it.
- `Tile::geometry_corner_radius` (T2) becomes the box's resolved radius, and
  **every** consumer reads it. The `self.window.geometry_corner_radius()` at
  `tile.rs:542-544` (T3) and `rendering.rs:271` (R1) must both be replaced. R1
  is currently right by accident and would become right by construction.
- The clipping radius applied to the client buffer (`tile.rs:1170`, via
  `clip_to_geometry`) stays a window-level concern, because it clips a surface,
  not a decoration. It should read the box radius masked to the content rect's
  corners, which under M1 is "zero on any side where a decoration sits between
  the content and the box edge" — i.e. always zero on top when a titlebar is
  present. That is already what T4 computes; it survives unchanged.

This is a restriction, not a removal, and it is the minimum that makes section
4 expressible. A radius that lives on the window cannot answer "am I the
outermost decoration of my box", because the window does not know what box it
is in.

## 7. `prefer-no-csd`

**The flag flip in `c70b8003` is a workaround and should be replaced.** Sway's
rule (`sway/xdg_decoration.c:64-90`, verified verbatim):

```c
enum wlr_xdg_toplevel_decoration_v1_mode mode =
    WLR_XDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE;
...
if (floating && client_mode) {
    mode = client_mode;
}
```

The mode starts at `SERVER_SIDE` unconditionally and the client's request is
honoured **only when floating**. `view_update_csd_from_client`
(`sway/tree/view.c:527-539`) likewise sets `B_CSD` only for floating
containers. Sway has **no configuration gate on the global**: it always
advertises `zxdg_decoration_manager_v1`.

swayward today:

- `swayward.rs:2507-2513` builds `XdgDecorationState::new_with_filter` on
  `ClientState::can_view_decoration_globals`, set from `config.prefer_no_csd`
  at `swayward.rs:2964`. A user who turns the flag off makes the global
  invisible and gets two titlebars back. **Sway has no such switch.**
- `handlers/xdg_shell.rs:1038-1050` grants whatever mode the client asks for,
  unconditionally, citing an SDL2 bug
  ([libsdl-org/SDL#8173](https://github.com/libsdl-org/SDL/issues/8173))
  that is now fixed.

### Decision

1. **Always advertise the global.** Drop the filter, matching sway. This is
   the part `c70b8003` only approximated by changing a default.
2. **Force `SERVER_SIDE` when tiled; honour the client only when floating.**
   Implement sway's rule in `request_mode`. The SDL2 comment describes forcing
   a *client-side* mode during window creation; forcing server-side is the
   opposite direction and the linked bug is fixed. Verify with a real SDL2
   client before relying on that reasoning.
3. **`prefer-no-csd` keeps its other job.** `utils/mod.rs:413-456` uses it to
   drive the xdg `tiled` state for clients that ignore xdg-decoration (GTK).
   That is a separate mechanism with no sway equivalent and no reason to
   change. The flag stops gating the global and keeps gating the tiled hint.
4. **Revert the shipped default?** No. Once (1) and (2) land the default value
   no longer controls double titlebars, so `prefer-no-csd true` in
   `resources/default-config.kdl` becomes a statement about the tiled hint
   only. Leaving it enabled is defensible on its own terms and reverting it
   would be churn. The three comments `c70b8003` rewrote should be corrected
   to stop claiming the flag controls decoration negotiation.

So: `c70b8003` is not wrong, it is *insufficient and mis-explained*. Keep the
config value, remove the gate it was standing in for, and fix the commentary.

Also flagged by the blocked task and confirmed here: `Mapped::has_xdg_decoration`
(`window/mapped.rs:376-378`) returns true when `decoration_mode.is_some()`,
which includes `SERVER_SIDE`. It is read at `tile.rs:1687,1692` to decide
whether `border csd` is permitted, so a server-side-decorated window currently
reports that it supports CSD. Under (2) every tiled window is server-side, so
this predicate becomes load-bearing for `border csd` on **every** window.
Resolve it in the same change.

## 8. Follow-up DAG

Created with `mu task add` and real `blocked-by` edges. Ordered so the tree
stays green at every step: the harness is fixed first because nothing after it
can be verified otherwise, and the two structural refactors are separated from
the behaviour changes that depend on them.

```
fix-decoration-matrix-nested-scenes-unfocused
        |
        +--> unify-tile-corner-radius-source          (fixes T3, the live defect)
        |            |
        |            +--> make-decorated-box-primary  (M1/M2, the refactor)
        |                        |
        |                        +--> corner-radius-owned-by-decorated-box  (section 4)
        |                        |            |
        |                        |            +--> resolve-dd14f07e-tab-seam
        |                        |            |
        |                        |            +--> retire-child-decorated-by-parent (G5)
        |                        |
        |                        +--> tab-strip-disables-inactive-child-borders (G7/M5)
        |
        +--> always-advertise-xdg-decoration-global   (section 7, independent)
                     |
                     +--> fix-double-titlebar-prefer-no-csd   (pre-existing)
```

Eight tasks. `always-advertise-xdg-decoration-global` shares no code with the
geometry work and can run in parallel from the start.

## 9. What this document does not cover

- **Floating windows.** Every measurement and every matrix scene is tiled.
  Sway's floating path differs (`container_set_geometry_from_content` asserts
  floating, and `xdg_decoration.c` honours client mode only when floating).
  `src/layout/floating.rs:1364` reads `tile.window().geometry_corner_radius()`
  and was not analysed.
- **`titlebar_border_thickness`.** B2 records that swayward has no equivalent
  of sway's titlebar border ring. Not designed here.
- **Marks.** Sway's titlebar reserves space for marks
  (`container.c:317-336`). Out of scope.
- **Shadows and the focus ring.** They consume the same radius (T3's
  neighbours at `tile.rs:558-585`) and will need the same treatment, but no
  scene isolates them.
- **Static reading only, for `src/render_helpers/`.** The border shader in
  `focus_ring.rs:117-190` was read, not executed. Its eight-rect decomposition
  with per-corner sizes is compatible with section 4, but that is a reading,
  not a measurement.
