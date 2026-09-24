# Fork base

swayward is a fork of niri, based on upstream main at commit 9e72e4917ca31baf4010496bf7f4aaf78d34d236 (2026-09-11).

Upstream source: the Git remote named `upstream`.

Full niri history is preserved, so `git blame` and `git log --follow` reach niri's original commits. That archaeology is the point: inherited backend code carries its original rationale.

This base is 93 commits *after* the `v26.04` tag, so it is a commit rather
than a release. That is deliberate: rebasing back onto `v26.04` would discard
upstream fixes we depend on. See [Upstream merge strategy](UPSTREAM.md).

To merge a later niri release (the next target must be a tag after 9e72e491):

    git fetch upstream --tags && git merge vXX.YY

See [Divergence from upstream niri](DIVERGENCE.md) for every edit to an
inherited file.
