# swayward

swayward is a Wayland compositor that combines niri's compositor foundation with
i3's nested container tree and sway's IPC protocol.

> [!WARNING]
> swayward is under active development and does not yet publish stable packages.
> Keep another compositor or desktop session available while testing it.

## Start using swayward

- [Build and start swayward](Getting-Started.md).
- [Configure swayward](Configuration:-Introduction.md).
- [Migrate a sway configuration](https://github.com/martintrojer/swayward/wiki/SWAY_CONFIG_MIGRATION).
- Check [sway compatibility](https://github.com/martintrojer/swayward/wiki/SWAY_COMPATIBILITY)
  and [known deviations](https://github.com/martintrojer/swayward/wiki/KNOWN_DEVIATIONS).
- [Connect sway IPC tools](IPC.md).

## Contribute

Read [Developing swayward](Development:-Developing-swayward.md) before changing
the compositor. The [design principles](Development:-Design-Principles.md)
describe the inherited architecture and project constraints.

The tracked files in `docs/wiki/` are the source of this wiki. Submit documentation
changes to the main repository instead of editing published wiki pages.
