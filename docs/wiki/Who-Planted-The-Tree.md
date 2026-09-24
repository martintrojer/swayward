swayward did not invent the nested container tree. Neither did sway. Neither,
strictly, did i3.

The idea has a lineage going back to the late 1980s, carried by a chain of
people who looked at the previous attempt and said “nearly, but I have notes.”

The names are almost all abbreviations, several are jokes, and at least one is
a pun on the display server it runs on. Window-manager history contains more
serious engineering than serious naming, which feels healthy.

## The short version

![The family tree: Plan 9 leads to wmi, wmii, i3, sway and swayward. i3 is where the tree is invented and sway moves it to Wayland. Ion and larswm feed into wmi, dwm is wmii's sibling, i3-gaps and SwayFX fork off i3 and sway, and niri joins swayward as a second parent from a different direction](_assets/shots/lineage-v1.svg)

With dwm, xmonad, awesome and others running alongside as cousins rather
than ancestors, and niri arriving from a different direction entirely.

## Bell Labs, and the idea that windows should not overlap

The ancestor everyone points at is **Plan 9 from Bell Labs**, the operating
system Rob Pike and others built after Unix. Bell Labs had already helped make
one operating system that escaped the laboratory; apparently this encouraged
them.

Its windowing systems are where the tiling instinct starts. **8½** — named
for the Fellini film, and because it succeeded a system called *mux* — tiled
windows rather than stacking them. Its successor **rio** went back to
overlapping windows, which is worth knowing because it undercuts any tidy
story about tiling simply winning: Pike moved to rio precisely because
users wanted more windows than fit on a screen at once.

The piece that mattered most for us is **acme**, Pike's text editor, which
is closer to a text-only windowing system: a hierarchical grid of columns
holding windows, everything visible, all of it keyboard and mouse driven
in an unusual way. acme is the direct inspiration the suckless family cites.

**What the names mean.** *Plan 9 from Bell Labs* is a joke about the film
*Plan 9 from Outer Space*, widely considered one of the worst films ever
made. *8½* is a Fellini reference. *rio* and *acme* are not acronyms.
Anyone claiming a straight-faced technical etymology here has not read the
room: this is a culture that names things for fun.

## The suckless family, and the arrival of tiling as a movement

The early 2000s produced a burst of tiling window managers for X11 — Ion,
larswm, ratpoison, PWM — each solving the arrangement problem differently.
The rectangles had unionised. Agreement on how to arrange them would take a
little longer.

**wmi**, *window manager improved*, began around 2003 and tried to combine
the best of larswm, Ion, evilwm and ratpoison. Its developers described it
as "the vi of window managers", because like vi it had a normal mode and a
command mode. That is the first appearance of an idea swayward still uses:
`mode` in a sway config is a direct descendant.

**wmii** is *window manager improved improved*, sometimes written
*improved²*. This is what happens when software engineers discover that naming
a successor is harder than writing one. It made the acme influence concrete
for X11: columns, tags instead of workspaces, and control through a virtual
filesystem, which is itself a very Plan 9 idea.

**dwm** — *dynamic window manager* — is a sibling from the same community,
famous for a self-imposed limit of 2000 lines of source. It is a cousin of
our lineage rather than a parent, but it is why "suckless" is a word you
will meet if you follow this history further.

## i3, and the tree itself

**i3** arrived on 15 March 2009, written by Michael Stapelberg. It was a
direct response to wmii: he liked the model but not the multi-monitor
support, the bugs, the stalled development, or the rc-based scripting.

The stated goals were clear documentation, proper multi-monitor support,
vim-like modes, and — the part this project exists for — **a tree
structure for windows**.

That is the invention, and it is why i3 deserves the credit even
though it did not start the tiling idea. Tiling had rectangles; i3 gave the
rectangles ancestry, siblings, and the occasional complicated relationship
with a parent. Earlier systems gave you *layouts*:
a master area and a stack, a set of columns, a fixed arrangement chosen
from a menu. i3 replaced the menu with a **tree** — a container holds
windows or other containers, recursively, and splitting is an operation on
that tree. Any arrangement you can describe, you can build, and it stays
built.

Every capability swayward advertises follows from that one decision.

**What the name means.** Here the trail goes cold, honestly. The common
explanation is that it stands for "improved tiling WM" or similar, but we
could not find a primary source from the author saying so, and this page
would rather admit a gap than pass on folklore. If you know of one, tell
us and we will cite it.

## sway, and the move to Wayland

**sway** arrived in 2015, written by Drew DeVault, with an unusual goal: not
a new model, but the *same* model on a new display server. It aimed to be a
drop-in replacement for i3 on Wayland, running your existing i3 config.

The name is a contraction of **S**irCmpwn's **Way**land compositor —
SirCmpwn being DeVault's handle. So sway is simultaneously a pun on
Wayland, an initialism, and a perfectly ordinary English word. As lineage
names go it is the most elegant one here, which is presumably why swayward
borrowed half of it.

sway also did the unglamorous work that gets no headlines: being the thing
people could actually rely on, and defining an IPC protocol stable enough
that a decade of bars, scripts and desktop integrations were written against
it. Reliability is rarely the exciting chapter in a lineage. It is the chapter
that lets all the later chapters happen. swayward speaks that protocol and
measures itself against sway's reliability.

## niri, from a different direction

**niri** (2024, Ivan Molodetskikh) is not in the tree lineage at all, and
that is the point of mentioning it.

It implements *scrollable* tiling — windows on an infinite horizontal
strip, inspired by the PaperWM GNOME Shell extension — which is a
different answer to the same question. It is also the foundation
swayward is built on: swayward is a fork of niri that replaces the layout
engine with the i3 tree and the IPC layer with sway's protocol.

So swayward has two parents that disagree about the central question. This is
less a family tree than a family argument rendered as a compositor, which is
probably why the result felt immediately at home here.

## The branches that leave the line

The tree did not travel in a single line. Several projects took it sideways,
and they are worth knowing about because one of them may be what you
actually want.

**i3-gaps** was the best-known i3 fork, adding gaps between windows and
other cosmetic features that i3 had declined. It is the happiest ending on
this page: in late 2022 the features were merged upstream and shipped in
i3 4.22, and the fork was archived. A fork that succeeds by ceasing to
exist is a rare thing, and swayward's own gap settings descend from that
work.

**SwayFX** is a sway fork that adds visual effects — blur, rounded corners,
shadows — and rebases on each sway release. It makes the opposite trade to
swayward: SwayFX keeps sway and changes the rendering, swayward keeps the
model and changes the foundation underneath it. If effects on real sway are
what you want, that is the shorter path, and `contrib/sway-to-kdl`
translates SwayFX configs as well as sway ones.

**hy3** is a Hyprland plugin by outfoxxed — who also maintains Quickshell —
that reimplements i3 and sway's manual tiling as a layout, complete with
node-based manipulation and tabbed groups. It is the clearest evidence that
the tree is an idea rather than an implementation: people want it badly
enough to rebuild it inside a compositor designed around a different model.

It is also the closest neighbour swayward has. If you already run Hyprland,
hy3 gets you the tree without changing compositor, and it is the shorter
path. The difference is where the model lives: hy3 is a layout plugin
inside a compositor with a different default, while in swayward the tree is
the compositor's own model and sway's IPC comes with it, so existing sway
bars and scripts work unmodified.

**Hyprland** itself belongs here as a cousin. Its default *dwindle* layout
splits the focused window automatically, which is dynamic tiling rather
than the manual tree, and it has pushed harder on effects and
configurability than anyone in this lineage.

**The dynamic tilers** — dwm, xmonad, awesome, and Hyprland's default —
are the other great branch of the family. They arrange windows by a rule
you choose rather than by a structure you build. Neither approach is a
lesser version of the other; they answer different questions, and
[Why care about window management](Why-Window-Management.md) works through
the distinction properly.

## swayward

Our name is sway plus *-ward*, meaning “in the direction of.” It is also the
ordinary English word for wandering off the expected path. A fork that keeps
sway's protocol, borrows niri's foundations, installs i3's tree, and then needs
a lineage diagram has earned the description. We drew the diagram anyway. It
seemed the least we could do for four decades of other people's work.

## What this lineage means for us

Two things worth stating plainly.

**The tree is not ours.** It is i3's, arrived at through wmii, through
acme, through a Bell Labs idea about visible windows. swayward implements
it, tests itself against i3's own test suite, and does not get to claim
the idea.

**Nobody in this chain invented from nothing.** Every project here looked at
its predecessor, kept the good bits, and moved the furniture. wmii kept acme's
columns. i3 kept wmii's modes and tags. sway kept i3's config format, deliberately, to the
point of running the same files. swayward keeps sway's protocol and i3's
model. That is not a lack of originality; it is how a good idea survives
long enough to reach you.

## Learn the model itself

If this page made the tree sound interesting, the next step is
[Cult of the Tree](Sway-School.md), which teaches it from first principles.

And if you want the argument for why any of this matters,
[Why care about window management](Why-Window-Management.md) makes the
case from scratch.

## Sources

- i3's origins and its response to wmii's limitations: the i3 project's own
  history, and the ArchWiki i3 page, which records the stated goals
  including the tree structure.
- wmii as *window manager improved²*: the wmii manual page, which spells it
  out in its NAME section.
- sway as SirCmpwn's Wayland compositor: the Debian and Gentoo package
  descriptions, which both expand it.
- acme's influence on dwm and wmii: the Plan 9 Foundation wiki, which
  describes acme as having "inspired (directly or indirectly) various X11
  window managers with a tabular view of graphical windows, such as dwm and
  wmii".
- niri's inspiration from PaperWM: niri's own README.
- i3-gaps merged into i3 4.22 and archived: the i3-gaps repository's own
  project-status notice.
- hy3 as an i3/sway-like manual tiling layout for Hyprland: the hy3
  repository description.

Corrections are welcome, particularly for the i3 name. This page prefers an
admitted gap to a confident guess.
