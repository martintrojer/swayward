For anyone who has never had a reason to think about it. No tiling experience
assumed, nothing to install.

## Nobody chose this

Look at your screen. Windows overlap, at sizes and positions decided by where
you once dragged them, what the application defaulted to, and chance. At least
one is a terminal you opened twenty minutes ago. Lost.

This feels like a fact of nature. It was a design decision from the early
1980s: a screen should work like a desk, with overlapping sheets you shuffle.
That was the right call when a screen showed two things. Then screens grew,
monitors multiplied, and two things became forty. The metaphor stayed. The
desk, to its credit, has remained calm about this promotion.

## The cost you have stopped noticing

- **Alt-Tab roulette.** You cycle, overshoot, cycle back, searching for
  something your computer already knows the location of.
- **Manual carpentry.** Dragging edges to fit two things side by side. Again
  tomorrow. Again after plugging in a monitor.
- **Lost windows.** Behind a browser, on another desktop, minimised in a
  previous decade. You open a second copy because finding the first is slower.
  The first copy is still out there somewhere, living a quiet life.
- **The reset.** Something knocks the arrangement over and you rebuild it by
  hand, because it lived nowhere except on the screen.

Each costs a couple of seconds, which is why none gets fixed. But they land at
the worst moment, right after you decide to do something, and put a small
hunt between every intention and every action. The losses just do not send
you an invoice.

## People who refused to accept it

Some stubborn people asked the obvious question: why is the computer not doing
this? It knows how many windows exist, how big the screen is and which one you
are using. It does not arrange them because in 1981 we told it not to, and
then stopped revisiting the instruction. Computers are very good at following
instructions, including that one.

So they wrote tiling window managers: every window gets space, nothing
overlaps, the computer does the arithmetic. [i3](https://i3wm.org/) mattered
most, because it got the model right. Instead of fixed layouts it gave you a
**tree**, containers holding windows or other containers, so any arrangement
you can describe, you can build, and it stays built.

With a tree, the four habits above stop existing rather than getting faster.
Nothing overlaps, so nothing is lost. You move by direction instead of cycling.
The computer does the geometry, including when you plug in a monitor. The
arrangement is a structure, not an accident. The best outcome is forgetting
that arranging windows was ever a job you did for free.

## Two kinds of tiling

"Tiling" means two different things, which is why arguments about it go
nowhere.

**Automatic tiling** means the window manager owns the arrangement. You pick a
layout, master-and-stack or spiral, and a rule slots new windows in. dwm,
xmonad and awesome work this way, and it is a good design: arranging is not
your job.

**Manual tiling** means you own it. Nothing overlaps and the computer still does
the geometry, but *where* the next window goes is something you said. i3, sway
and swayward work this way.

A rule has to be simple enough to predict, so it cannot express much.
Master-and-stack answers "one important thing and some others". It has no
answer to "two logs stacked on the right, a browser on the left, three
terminals as tabs in the corner, and keep it that way". And because the layout
is a function of what is open, it reshuffles whenever that changes. Open a
browser and everything moves. Close it and everything moves back, like a
dinner table standing up for one late guest.

Manual tiling asks you to decide where each window goes. That is a small tax,
and for someone who does not care where things land it buys nothing; automatic
tiling is the right answer for them. For everyone else, the tree is how you
express *any* arrangement rather than pick from a menu. Because you built it,
it does not depend on what is open, so it holds still. Close a window and its
siblings take the space; open one and it goes where you said.

## The honest part

There is a learning curve of about a week, and manual tiling adds a little,
because you are learning a model rather than a list of shortcuts. Image editors
and anything with floating palettes want to float, which is why every serious
tiling compositor supports floating windows, swayward included. And a
keyboard-driven tree is a preference, not the correct answer. Plenty of
productive people use a conventional desktop and are doing fine.

If overlapping windows have never bothered you, you can stop here with our
blessing. This page is for people who have felt the friction without a name
for it.

## If you are curious

You can learn the tree without installing anything:
[Cult of the Tree](Sway-School.md) teaches it from first principles in short
lessons with quizzes.

If you love it, swayward is one way to get it, and [i3](https://i3wm.org/) and
[sway](https://swaywm.org/) are excellent and much older ones. If you want
something other than a tree, [niri](https://github.com/YaLTeR/niri) does
scrollable tiling beautifully.

Learning the model is the win. Where you take it afterwards is up to you.
