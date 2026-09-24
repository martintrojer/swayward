# Public sway config corpus audit

This bounded audit sampled 12 public configuration entry points pinned in
[`contrib/sway-to-kdl-corpus.json`](../../contrib/sway-to-kdl-corpus.json). It
covers vanilla and SwayFX configs in monolithic and modular forms. The oldest
sample is from 2021. `luispabon-control` is the near-default control.

The manifest records repository revisions, source paths, declared SPDX licenses
(or `none`), raw SHA-256 hashes, and normalized hashes. The normalized form
removes comments and blank lines and collapses whitespace. None matches the
normalized sway 1.11 default hash in the manifest. The repository contains no
config text from this corpus.

## Run the audit

The runner is opt-in and is not called by CI or the normal translator tests.
Its first run needs network access:

```sh
contrib/sway-to-kdl-corpus --fetch --swayward target/debug/swayward
```

Later runs omit `--fetch` and use `.cache/sway-to-kdl-corpus`, which is ignored
by git. The runner checks out exact commits, verifies both hashes, and translates
under a temporary `HOME` and XDG directory set. It only reads config files;
`exec` directives become KDL text and are never run.

Result from 2026-09-24 after the two fixes in this audit:

| Entry | Exit | Manual attention | Parse | Validate | Classification |
|---|---:|---:|---|---|---|
| alebastr-main | 0 | 22 | ok | ok | unresolved environment or include |
| alebastr-bindings-module | 0 | 26 | ok | ok | expected manual attention |
| ammgws-main | 0 | 43 | ok | ok | unresolved environment or include |
| ammgws-rules-module | 0 | 16 | ok | ok | expected manual attention |
| codebam | 0 | 155 | ok | ok | unresolved environment or include |
| aruyu-swayfx | 0 | 112 | ok | ok | unresolved environment or include |
| owpk-main | 0 | 3 | ok | ok | unresolved environment or include |
| owpk-effects-module | 0 | 3 | ok | ok | expected manual attention |
| owpk-rules-module | 0 | 28 | fail | fail | validation failure |
| alicin-old | 0 | 41 | ok | ok | unresolved environment or include |
| luispabon-control | 0 | 3 | ok | ok | unresolved environment or include |
| vaelixd-swayfx | 0 | 19 | ok | ok | unresolved environment or include |

`parse` means parsing through swayward's typed KDL config loader. `validate`
is the full `swayward validate` result; the values are identical for this
corpus because validation found no post-parse errors.

## Findings

Two small defects were fixed with local regressions:

- `client.focused $border #223344 $text ...` emitted unresolved variables into
  typed color fields and failed validation. The translator now reports the
  directive instead of emitting invalid colors.
- `input type:pointer { scroll_button BTN_SIDE }` emitted a bare KDL identifier.
  The translator now maps Linux button names to their numeric event codes.

One remaining defect has follow-on task
`sway-to-kdl-json-escaped-nonascii`:

```sway
for_window [app_id="firedragon" title="firedragon — Sharing Indicator"] kill
```

Python's default JSON escaping produces `\\u2014`, which KDL rejects. A second
follow-on, `sway-to-kdl-named-scroll-button`, requests typed Rust coverage for
the named scroll-button fix.

Manual inspection was limited to the declared high-risk list. Backslash
continuations remained intact in `alebastr-main` and `alicin-old`. Grouped
`set`, `bindsym`, and command blocks in `ammgws-main` were reported for manual
conversion rather than silently discarded. Include globs preserve lexical
ordering; command substitutions and missing or sterile-environment paths are
reported. Variable-bearing includes were reported where they could not resolve
in the sterile environment. Binding command chains containing `;` and `,`
remained quoted command strings. Criteria quoting and regexes remained visible,
except for the non-ASCII escaping defect above. SwayFX directives either mapped
to typed effects or appeared in manual-attention output.

This audit did not establish general semantic equivalence. It inspected only
those high-risk constructs in these 12 pinned entry points. Other manual
comments were checked for source visibility but not adjudicated directive by
directive.
