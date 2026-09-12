# Fork base

swayward is a fork of niri, forked at tag v26.04, commit 8ed0da44d974c32c6877d2f4630c314da0717ecb.

Upstream: https://github.com/niri-wm/niri.git (remote name `upstream`)

Full niri history is preserved, so `git blame` and `git log --follow` reach niri's original commits. That archaeology is the point: inherited backend code carries its original rationale.

To merge a later niri release:

    git fetch upstream --tags && git merge vXX.YY

See docs/DIVERGENCE.md for every edit we have made to inherited files.
