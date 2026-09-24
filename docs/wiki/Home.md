swayward is what happens when somebody looks at niri's compositor foundation,
i3's nested container tree, and sway's IPC protocol and says, with entirely too
much confidence, “yes, those belong in one process.” They do, it turns out.
This wiki is the paperwork.

> [!NOTE]
> swayward is beta. Keep another compositor or desktop session available so
> that you can recover from a startup or display failure.

## Pick a door

You do not need to read this wiki in order. We tried that once; it became a
small book and developed opinions.

- [Get started](Getting-Started.md): install swayward and start a session.
- [Cult of the Tree](Sway-School.md): learn the container tree in five short induction classes.
- [Configure swayward](Configuration:-Introduction.md).
- [Migrate a sway configuration](https://github.com/martintrojer/swayward/wiki/SWAY_CONFIG_MIGRATION).
- Check [sway compatibility](Sway-Compatibility.md) and
  [differences from sway](Differences-from-Sway.md).
- [Connect sway IPC tools](IPC.md).

## Bring tools

The induction classes are the fun part. This is the part where the rectangles are Rust, and someone still has to read the diff.

Read [Developing swayward](Development:-Developing-swayward.md) before changing
the compositor. The [design principles](Development:-Design-Principles.md)
describe the inherited architecture and project constraints.

The tracked files in `docs/wiki/` are the source of this wiki. Submit documentation
changes to the main repository instead of editing published wiki pages.
