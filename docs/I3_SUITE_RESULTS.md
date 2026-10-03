# Reading the i3 suite results

This page explains one part of swayward's test suite. [Testing and
conformance](https://github.com/swayward-wm/swayward/wiki/Testing-and-Conformance)
describes the whole suite.

The oracle runs i3's own tests unchanged against i3, sway, and swayward. This is
a demanding way to find regressions in the shared tree and IPC model. It is not
a sway compatibility percentage: many assertions require i3's X11 window
manager, config parser, bar, restart mechanism, or private tree shapes.

The raw count mostly measures "is this i3?" Sway 1.12 has 2,197 non-passes,
swayward has 2,302, and they share 1,960 of them.

The figures below come from the oracle revision pinned in
[`tests/oracle.toml`](../tests/oracle.toml).
Run this command to reproduce them from an oracle checkout:

```sh
./contrib/i3-suite-summary /path/to/sway-ipc-oracle
```

## Compare the same assertions

The two snapshots cover 3,755 assertions. Sway 1.12 has 2,197 non-passes, and
swayward has 2,302. Both have a non-pass on 1,960 of the same assertions.
Swayward alone has 342, while sway alone has 237. These counts include failures,
skips, and assertions not reached after an earlier abort.

The useful question is not whether a Wayland compositor reproduces every i3
implementation detail. It is what explains the 342 assertions that sway passes
and swayward does not.

The five files with the largest gaps account for 111 of those assertions. Every
one requires X11 identity, window type, urgency hints, requested geometry, or
client geometry that does not cross swayward's `xwayland-satellite` boundary.
After removing those reviewed X11-boundary assertions, swayward passes **1,216
of the 1,447 assertions that sway passes (84%)**. The remaining sway-relative
count is 231.

This filter is deliberately narrow. It does not claim that every remaining row
is a product bug, or that these five files contain every X11-bound assertion.
It states only what the black-box review established. The
[X11 window identity](KNOWN_DEVIATIONS.md#x11-window-identity) section and the
[Xwayland guide](https://github.com/swayward-wm/swayward/wiki/Xwayland) explain
the design: the satellite presents X11 clients as ordinary `xdg_toplevel`
surfaces, without a separate XID, class, instance, role, or window type. Work on
standard protocols that can carry more of this metadata is planned after beta 1.

The oracle's
[classification file](https://github.com/swayward-wm/sway-ipc-oracle/blob/main/i3/classifications/swayward-54c5acd9.toml)
records the assertion-level review. Of the 342 swayward-only non-passes, 41
currently use swayward finding families. Those are compatibility findings, not
exceptions hidden by the comparison, and they remain bugs or harness problems
to investigate. Satellite families account for 236 rows; 65 are other or
unclassified rows. The 111-row manual X11 review overlaps these categories
because some geometry rows remain labelled as findings.

## Use the sway corpus for sway compatibility

The oracle also compares IPC requests and events directly with sway 1.12. This
is the closer measure of sway compatibility because each scenario starts from
the same state and compares sway-shaped replies or event sequences.

The current snapshot records:

| Corpus | Match | Mismatch | Not applicable |
| --- | ---: | ---: | ---: |
| Hand-picked scenarios | 749 | 31 | 0 |
| Events | 44 | 1 | 0 |
| Command fuzz | 195 | 0 | 0 |
| Random sequences | 329 | 171 | 0 |
| i3-derived scenarios | 4,118 | 62 | 0 |
| Wire fuzz | 6 | 4 | 0 |

Read these as measurements by scenario, not as a ranking or one combined
percentage. The [sway compatibility guide](SWAY_COMPATIBILITY.md) describes the
implemented requests, commands, events, and known limits.
