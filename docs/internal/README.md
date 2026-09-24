# Internal documents

Working documents for people building swayward. They are not user
documentation, they are not published to the wiki site, and they are not part
of any release artifact.

They live in the repository on purpose. A design decision that is not written
down gets re-litigated, and a voice that is not written down drifts. Keeping
them beside the code means they are versioned with it and reviewed with it.

| Document | What it is for |
| --- | --- |
| [`STANCE.md`](STANCE.md) | What swayward is, what it values, and how it asks to be judged. |
| [`specs/`](specs) | Design records. The foundation spec is the design of record for the fork. |

Anything user-facing belongs in [`docs/wiki/`](../wiki), which is published,
or in the top-level docs that packaging and contributors consume
(`BUILDING.md`, `KNOWN_DEVIATIONS.md`, `SWAY_COMPATIBILITY.md`,
`UPSTREAM.md`, `DIVERGENCE.md`, `FORK-BASE.md`).

Internal does not mean secret. The repository is public and these files are
readable by anyone who goes looking. It means they are addressed to
maintainers rather than users, and are free to be blunt about unfinished work.
Do not put credentials, personal data, or anything embarrassing-if-quoted
here; `docs/internal/` is a filing decision, not a privacy boundary.

## Public documents must not link here

**Nothing user-facing may reference `docs/internal/`.** Not the README, not
the wiki, not `KNOWN_DEVIATIONS.md`, not code comments, not test fixtures.

A link inward sends a reader from a finished page to a working document that
assumes context they do not have, may describe work that was never done, and
is free to change without warning. It also makes the split meaningless: a
document reachable in one click from the front page is published, whatever
directory it sits in.

When a public page needs a fact that lives in a spec, **state the fact** in the
public page. A spec is where a decision is worked out; a public document says
what was decided. If the reasoning genuinely matters to users, it has outgrown
`docs/internal/` and should be rewritten as a wiki explanation page.

Two files are allowed to link inward, because both address maintainers:
this README, and [`AGENTS.md`](../../AGENTS.md).

The check is a grep, and `contrib/check-internal-links` runs it:

    rg -l 'docs/internal' --glob '!docs/internal/**' --glob '!AGENTS.md'
