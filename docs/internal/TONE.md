# Tone

How swayward talks about itself, everywhere: README, wiki, release notes, the
launch posts, issue replies, and answers to criticism.

This is an internal working document. It is not marketing copy and it is not
published as-is. Tasks reference it so the project sounds like one project
instead of re-deciding its voice on every page.

**This page owns how swayward sounds. [`STANCE.md`](STANCE.md) owns what it
says.** When the two overlap, state the fact there and the delivery here. If
they ever disagree, STANCE.md wins on substance and this page wins on
wording.

The short version: **curious and happy, funny about ourselves, never funny at
anyone else's expense, and dry wherever the evidence lives.**

## Why bother writing this down

A consistent voice is what lets someone other than the maintainer speak for the
project. That is the difference between a one-person project and a shared one.
It also settles arguments in advance, so a reply to a hostile comment does not
have to be improvised at the moment it is least fun to improvise.

## The five points

1. **Positive.** swayward exists because the nested tree is worth having on a
   modern compositor. That is a happy reason. Never frame swayward as a
   reaction against anything, and never sound aggrieved.

2. **Open about AI-guided development.** Stated early, plainly, and without
   embarrassment — and without evangelism either. The line is "here is how it
   was built, here is how you can check it." Never "look what AI can do." The
   position itself, including why disclosure is project-level rather than
   per file, is in [`STANCE.md`](STANCE.md#on-being-built-with-ai).

3. **Non-adversarial.** swayward does not compete with niri, sway, i3 or
   Hyprland. No benchmarks against them, no "unlike X", no implying anyone made
   a wrong choice. Different models suit different people, and saying so is
   simply true.

4. **Celebrate the tree, and teach it.** This is the only evangelism. Show what
   the tree makes possible rather than asserting that it is good. A reader who
   learns the tree and then picks a different compositor is a win.

5. **Credit niri, sway and i3 by name, warmly.** What we owe each of them is
   listed in [`STANCE.md`](STANCE.md#what-it-owes); the rule here is that it
   is said warmly and by name, in the reader's path rather than in a footnote.
   It is the interesting part of the story, not a licence obligation.

## Humour

Humour is wanted. A flat voice cannot celebrate anything, and enthusiasm
delivered deadpan reads as a manual.

The reference vibe is **"Cult of the Tree — Induction Class 1"**: playful,
self-aware, faintly ceremonial, enjoying its own enthusiasm. Straight-faced
delivery beats winking.

### Self-deprecation is the engine

Aim the joke inward. It is funnier, and it does the non-adversarial work for
free: laughing at our own zeal is structurally incapable of insulting anyone
else.

Two rich targets, both true:

- **The zeal.** A project this committed to nested containers is a little
  absurd, and knows it.
- **The AI provenance.** Being built by a swarm of agents is funny. Owning the
  joke is also the strongest possible position, because a joke you have already
  made cannot be used against you. The humour must sit on top of real evidence,
  never instead of it — self-deprecation about AI works only while the i3 oracle
  is unmodified and the numbers check out.

Never joke about AI provenance in a way that implies nobody checked. Someone
did, and the claim is checkable. The joke is about the *process being strange*,
not about the *result being untrustworthy*.

### Hard lines

- **Never aimed at another project or its users.** Not sway, not niri, not i3,
  not Hyprland, not scrolling layouts, not floating layouts, not the people who
  like them. Gentle teasing included. This one has no exceptions.
- **Never at a user's expense.** Not their config, their hardware, their
  distro, or their question.
- **The joke is the frame, never the content.** A lesson's instruction is plain
  and correct. Comedy inside an instruction costs the reader time.
- **Never in-jokey enough to exclude a newcomer.** Someone who does not find it
  funny should still learn the tree and never feel shut out.
- **Keep it legible to non-native English speakers.** A pun that needs decoding
  fails.

## README terse, wiki narrative

The two live at different temperatures, on purpose.

**The README is terse and technical.** Someone lands there to find out what
this is, whether it works, and how to install it. Short sections, real
numbers, no throat-clearing. The beta-tester callout is the one place it
relaxes, because it is asking for something rather than stating something.

**The wiki is narrative and quirky.** A reader who clicked into the wiki has
already decided to spend time here, and a page that reads like a specification
will lose them. This is where the voice lives: opinions, asides, a story
instead of a list, and jokes that do not apologise for themselves.

## Write with spice, not with polish

The failure mode of careful writing is that it is *clear, correct and
forgettable*. Balanced sentences, hedged claims, every paragraph the same
length. Nothing is wrong with it and nobody finishes it.

What to do instead:

- **Name the characters.** "The i3 wizards" beats "the developers of i3". A
  piece of software written by stubborn people who refused to accept
  overlapping windows is a better story than a feature, so tell it that way.
- **Have an opinion out loud.** "Alt-Tab roulette is not a workflow" says more
  than three balanced sentences about switching costs. State it, then be fair
  about the exceptions — conviction first, caveat second, never the reverse.
- **Vary the rhythm.** A four-word sentence after a long one is the cheapest
  emphasis available. Prose at a constant pace reads as a manual.
- **Let enthusiasm show.** If nested containers are delightful, say delightful.
  Hedging the enthusiasm out of a page about a thing we love is a strange way
  to persuade anyone to love it.
- **Keep the aside.** The parenthetical that made you smile while writing it is
  usually the sentence a reader will quote.

The discipline is that spice sits on top of correct instruction, never in
place of it. A funny sentence that leaves the reader unsure which key to press
has failed at the only job that mattered.

One page resists this deliberately:
[Why care about window management](../wiki/Why-Window-Management.md) plays it straight,
because it is aimed at someone who does not yet know there is anything to be
enthusiastic about, and in-group humour reads as a clique from outside. Earn
the reader first, then be funny at them.

## Where the voice changes

The register is not uniform. It follows what the text is doing.

| Surface | Register |
| --- | --- |
| README | Terse and technical. Dry wit at most; never a bit. |
| Wiki generally | Narrative and quirky. This is where the voice lives. |
| Tutorial, wiki landing, the invitation to try it | Playful. The Cult voice lives here. |
| Conformance numbers, deviation ledger, invariants, provenance claims | Dry. No jokes. |
| Issue replies, bug reports, anything from a frustrated user | Warm and plain. Humour is a risk when someone is annoyed. |
| Anything addressed to or about upstream | Straight and respectful. |

The page can be funny **because** the evidence is not. Never joke about
correctness, stability or test coverage; those carry the credibility, and a
reader who suspects we are being cute about a number will not check the rest.

## Answers we have already decided

These live in [`STANCE.md`](STANCE.md#settled-answers), not here. That page
owns what the project says; this one owns how it sounds.

Deliver them plainly and without defensiveness, in the order STANCE.md gives,
which is the order a stranger actually asks.

## The test

Before publishing anything:

1. Would a niri or sway maintainer reading this feel respected?
2. Would a curious stranger come away having learned something about the tree,
   even if they never install swayward?
3. Is every joke aimed at us?
4. Is every number checkable, and checked?

Any "no" means rewrite.

## See also

- [`docs/internal/specs/2026-09-12-swayward-foundation.md`](specs/2026-09-12-swayward-foundation.md)
  — "The zen of swayward" and "Who it is not for", which this tone serves.
- [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — the AI-assisted contributions policy.
- [`docs/KNOWN_DEVIATIONS.md`](../KNOWN_DEVIATIONS.md) — the evidence the dry
  register protects.
