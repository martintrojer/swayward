# Contributing to swayward

Open a GitHub issue if you have questions or want to discuss anything:
https://github.com/martintrojer/swayward/issues
GitHub Discussions is not enabled, so issues are the only channel.

## Issues

This is a good way to help many new and existing users without programming knowledge.

- Answer and help people in GitHub issues.
- Check and point out duplicate issues.
- Check for issues that are likely application bugs (and not swayward bugs).
    - Ask or try to reproduce on another non-Smithay-based compositor (sway, KDE/KWin, GNOME/Mutter). If the issue reproduces, it's likely an application bug.
    - Ask or try to reproduce on another *Smithay-based* compositor ([cosmic-comp], [anvil]). If the issue reproduces only on Smithay compositors, it may be a Smithay bug.
    - Make sure you're testing the Wayland version of the app on all compositors. Apps may silently use X11 when an X11 `$DISPLAY` is available.
    - Problems with X11 apps should be reported to [xwayland-satellite]. When testing xwayland-satellite on different compositors, make sure you use xwayland-satellite's `$DISPLAY` (rather than another compositor's built-in Xwayland `$DISPLAY`).
    - After testing, mention where you could and couldn't reproduce, as well as the exact steps to reproduce if the issue is missing them.
- Try to reproduce the issue on your own system and write if you could or couldn't reproduce it.
- Upvote issues with a thumbs up reaction as you like.
- Ideas and feature requests also go to issues.

If your issue is a duplicate, or not a swayward issue (application bug, hardware problem, configuration problem), then please close it.

## Reviewing and testing pull requests

Testing and reviewing pull requests is useful help, and swayward is
small enough that a second pair of eyes makes a visible difference.

### Testing

Pick a pull request you like, then build it and give it a go.
The [Developing swayward wiki page](https://github.com/martintrojer/swayward/wiki/Development:-Developing-swayward) has guidance on running swayward test builds.

Be really thorough with your testing.
We're striving for polished features in swayward, so point out any issues and bugs, even small ones like animation jank.

- Think of weird edge cases or unexpected interactions and try them to see that they work reasonably.
- Try to break the feature and check that it behaves well.
- Where applicable, try different input devices: keyboard, mouse, trackpad, tablet, touchscreen.
- Watch out for any new performance drops.

For bug fixes, first make sure you can reproduce the bug, then do the same steps in the PR test build, and verify that the bug is fixed.
Be similarly thorough: test any similar or related edge cases to verify that the fix doesn't introduce any new problems.

Write your findings in the pull request: any issues you found, or if everything worked well.
Re-test after the author updates the code to see that your issues were fixed.

Don't hesitate to test even if someone else already did; very frequently different people will stumble upon different problems.

### Reviewing

Reviewing is time-consuming and is where help goes furthest.
Anyone with code accepted into swayward is welcome, but this is not a requirement; even if you aren't familiar with Rust you may find some logic problems.

Pick a pull request, then review its code.

- Check that everything looks good, check various conditions for edge cases.
- See if there are any scenarios the author forgot to handle.
- Check that the code fits well into the rest of swayward, follows its design and code style.
    - That is vague on purpose. Look at the surrounding code and at similar modules (e.g. when implementing a new protocol, check other protocol implementations), and follow the style and structure you find.
- Check for unrelated changes that may be better split into their own pull request.
- Check that the wiki had been updated if necessary (for example, new config options were documented with examples, and have a correct Since annotation).

Point out everything you find as review comments (don't forget to submit the review).
Be constructive and respectful; some people may be new to programming and Rust.
As the author addresses the comments and issues, check the code again to see that the problems were fixed.
If everything looks good, say that, so it is visible that someone has reviewed the PR.

As with testing, don't hesitate to look through and comment even if someone else already had.
Extra pairs of eyes catch more problems.

## Writing pull requests

- Make sure new features align with swayward's design direction. Ideally, there should be an existing issue or discussion where we settled on that solution.
- Keep pull requests focused on a single feature or bug fix with no unrelated changes.
- Try to split your changes into small, self-contained commits. Every commit should build and pass tests. This makes it much easier to review your PR, and bisect for regressions in the future.
    - When addressing PR comments, try to squash the changes straight into the relevant commits.
    - In some cases when the requested changes are big/unclear, you can leave them as separate commits on top, but please squash and otherwise clean up the history when the changes are finalized.
    - To update the main branch, please rebase instead of merging. Try to force-push the main update rebase separately from other changes, this way it's easy to skip during review since it's usually not interesting.
    - For bigger features, starting with one messy commit and gradually splitting self-contained changes out of it as the code settles works well.
    - [git-rebase.io](https://git-rebase.io/) is a helpful guide for splitting commits and cleaning up history in git.
- When you address a review comment, mark it as resolved.
- Remember to [run tests](https://github.com/martintrojer/swayward/wiki/Development:-Developing-swayward#tests) and format the code with `cargo +nightly fmt --all`.
- For new layout actions, remember to add them to the randomized tests. For weird Wayland handling, adding client-server tests in `src/tests/` could be very useful.
- Test your changes by hand thoroughly, including for edge cases and weird interactions. See the Testing section above for some tips.
- Remember to document new config options on the wiki.
- When opening a pull request, ensure "Allow edits from maintainers" is enabled, so a maintainer can make final tweaks before merging.

### How to get your pull request reviewed more quickly

- Make it small and self-contained. Avoid mixing several unrelated changes in one PR.
- Split the PR into small and self-contained commits. This makes it much easier to review.
- Discuss new features, options, or behavior changes beforehand; make sure there's consensus about the design.
- When creating the pull request, clearly write what it does, what problem it solves, how to test it.
- Follow the rest of the advice from this document.

## Writing documentation

swayward's docs have a voice, and it varies with what the page is doing.

- The README is terse and technical. The wiki is where the voice lives: opinions, asides, a story instead of a list.
- Conformance numbers, the deviation ledger, invariants and provenance claims stay dry. The page can be funny because the evidence is not, so never joke about correctness, stability or test coverage.
- Issue replies and anything to or about upstream are warm and plain.

Three rules hold everywhere, with no exceptions:

- **Never aim a joke at another project or its users.** Not sway, niri, i3, Hyprland, SwayFX, hy3, scrolling or floating layouts, or the people who like them. Aim it at ourselves.
- **Never at the reader.** Not their config, hardware, distro or question.
- **The joke is the frame, never the content.** It may open or close a section. It never sits inside a key table, a command, a KDL block or a sentence that tells the reader which key to press. Instructions stay plain and correct.

Keep it legible to non-native English speakers, and never so in-jokey that a newcomer feels shut out.

## AI-assisted contributions

Most of swayward was written with AI assistance, so it would be incoherent to
refuse contributions made the same way. AI-assisted work is welcome here.

The bar is the same for everyone, and it is about evidence rather than
authorship:

- Claims need executable proof. A feature needs a test that fails without it;
  a bug fix needs the reproduction that was red before and green after. "The
  suite passes" is a claim about a command you ran, not a substitute for it.
- Behaviour that differs from sway needs a citation into sway's source, and a
  line in [`docs/KNOWN_DEVIATIONS.md`](docs/KNOWN_DEVIATIONS.md) saying why.
- Never edit the unchanged i3 tests in
  [`sway-ipc-oracle`](https://github.com/martintrojer/sway-ipc-oracle). Swayward
  pins them in `tests/oracle.toml`; a test you can edit to pass is not evidence.
- Read what you submit. Unverified output wastes a reviewer's time whether a
  model or a person produced it, and reviewers here are scarce.

swayward follows the Linux kernel's rule here: an AI does not sign off on a
contribution. A human reviews it, checks the licensing, and is responsible
for the result
([`coding-assistants.rst`](https://www.kernel.org/doc/html/next/process/coding-assistants.html)).
Disclosure is project-level rather than per file, because every file here
was touched the same way and a per-file marker would either mean nothing or
imply the unmarked ones were written by hand.

See [`AGENTS.md`](AGENTS.md) for the working conventions this project uses,
including the ones learned by getting something wrong first.

### Upstream projects

Pull requests from swayward to niri or sway are very unlikely. The layout
engine and IPC layer are replaced wholesale, so there is little here that
would apply upstream in the first place.

Where something would, it does not travel as an AI-generated pull
request. niri's contributing guide asks that mostly-LLM-generated pull
requests not be sent there, and that request is respected on their repository
regardless of how this one is built. Report it as an issue instead, or let
someone rewrite it by hand and stand behind it, saying where it came from.


[cosmic-comp]: https://github.com/pop-os/cosmic-comp
[anvil]: https://github.com/Smithay/smithay/tree/master/anvil
[xwayland-satellite]: https://github.com/Supreeeme/xwayland-satellite
