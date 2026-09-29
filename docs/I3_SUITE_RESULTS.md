# Reading the i3 suite results

The oracle runs i3's own tests unchanged against i3, sway, and swayward. This is
a demanding way to find regressions in the shared tree and IPC model. It is not
a sway compatibility percentage: many assertions require i3's X11 window
manager, config parser, bar, restart mechanism, or private tree shapes.

The raw count mostly measures "is this i3?" Sway 1.12 has 2,197 non-passes,
swayward has 2,277, and they share 1,936 of them.

The figures below come from oracle commit
[`a1ba2d6`](https://github.com/martintrojer/sway-ipc-oracle/tree/a1ba2d6).
Run this command to reproduce them from an oracle checkout:

```sh
./contrib/i3-suite-summary /path/to/sway-ipc-oracle
```

## Compare the same assertions

The two snapshots cover 3,755 assertions. Sway 1.12 has 2,197 non-passes, and
swayward has 2,277. Both have a non-pass on 1,936 of the same assertions.
Swayward alone has 341, while sway alone has 261. These counts include failures,
skips, and assertions not reached after an earlier abort.

The useful question is not whether a Wayland compositor reproduces every i3
implementation detail. It is what explains the 341 assertions that sway passes
and swayward does not.

The five files with the largest gaps account for 111 of those assertions. Every
one requires X11 identity, window type, urgency hints, requested geometry, or
client geometry that does not cross swayward's `xwayland-satellite` boundary.
After removing those reviewed X11-boundary assertions, swayward passes **1,217
of the 1,447 assertions that sway passes (84%)**. The remaining sway-relative
count is 230.

This filter is deliberately narrow. It does not claim that every remaining row
is a product bug, or that these five files contain every X11-bound assertion.
It states only what the black-box review established. The
[X11 window identity](KNOWN_DEVIATIONS.md#x11-window-identity) section and the
[Xwayland guide](https://github.com/martintrojer/swayward/wiki/Xwayland) explain
the design: the satellite presents X11 clients as ordinary `xdg_toplevel`
surfaces, without a separate XID, class, instance, role, or window type. Work on
standard protocols that can carry more of this metadata is planned after beta 1.

The oracle's
[classification file](https://github.com/martintrojer/sway-ipc-oracle/blob/a1ba2d6/i3/classifications/swayward-eb170906.toml)
records the assertion-level review. Of the 341 swayward-only non-passes, 240
currently use a `swayward_*_finding` family. Those are compatibility findings,
not exceptions hidden by the comparison, and they remain bugs or harness
problems to investigate. The classification also has 78 rows in explicit
`xwayland_satellite_*` families and 23 other or unclassified rows. The 111-row
manual X11 review overlaps these categories because some geometry rows remain
labelled as findings. The follow-up classification refresh will move those
X11-bound rows out of the finding families and shrink the 240 figure without
changing any measured outcome.

## Use the sway corpus for sway compatibility

The oracle also compares IPC requests and events directly with sway 1.12. This
is the closer measure of sway compatibility because each scenario starts from
the same state and compares sway-shaped replies or event sequences.

The current snapshot records:

| Corpus | Match | Mismatch | Not applicable |
| --- | ---: | ---: | ---: |
| Hand-picked scenarios | 499 | 51 | 0 |
| Events | 32 | 13 | 0 |
| Command fuzz | 24 | 0 | 0 |
| Random sequences | 256 | 244 | 0 |
| i3-derived scenarios | 3,658 | 522 | 0 |
| Wire fuzz | 5 | 5 | 0 |

Read these as measurements by scenario, not as a ranking or one combined
percentage. The [sway compatibility guide](SWAY_COMPATIBILITY.md) describes the
implemented requests, commands, events, and known limits.
