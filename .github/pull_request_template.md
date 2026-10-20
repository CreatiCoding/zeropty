<!-- PR title = the squash merge commit message: use the Conventional Commits format. Example: "feat(cli): add --version" (CONTRIBUTING.md) -->

## What changed and why

<!-- What you changed and why. If there is a related issue: Fixes #number -->

## What I checked

- [ ] `scripts/check.sh` passes (build, tests, clippy, rustfmt, rustdoc, examples, MSRV, commits and changelog; see Pull requests in CONTRIBUTING.md)
- [ ] I broke the new code path on purpose and saw a test fail (if applicable)
- [ ] `capture-pane` output stays byte-identical for the same screen (unless changing it is the point)

## Version and changelog

- [ ] Commits and the PR title follow Conventional Commits, and breaking changes are marked with `!` / `BREAKING CHANGE:` (what counts as breaking: docs/VERSIONING.md)
- [ ] For a user-visible change, I added a line under `## [Unreleased]` in CHANGELOG.md and the same entry in Korean in docs/ko/CHANGELOG.md (if not, I explained above why users do not see it)
- [ ] If I changed the C ABI or the Node exports, I updated `api/*.txt` (`node scripts/check-api.mjs --update`). A line that disappears is a breaking change
- [ ] If I changed documentation, the Korean copy (README.ko.md, docs/ko/) has the same change and `node scripts/check-links.mjs` passes

## Performance

<!-- Does this affect memory, CPU or response time? If so, attach the before and after numbers measured interleaved with tmux,
     with the commands, and update docs/BENCHMARKS.md (and README, for items at 10x or better) in the same PR. -->

- [ ] No effect on performance
- [ ] Affects performance (engine tick, output handling, screen emulator, capture, ...): the measurements and the docs/BENCHMARKS.md update are included (multipliers divide the ends of the ranges and round down, as in "How multipliers are computed" in docs/BENCHMARKS.md)
- [ ] If I redrew README images, they have new names and `sh docs/assets/cdn-check.sh` passes

## Public repository check

- [ ] I looked at `git diff --cached` and there are no secrets, tokens, personal paths or names, or unchecked session recordings
