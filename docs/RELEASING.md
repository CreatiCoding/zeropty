# Releasing

**English** · [한국어](ko/RELEASING.md)

[README](../README.md) · [Docs](README.md) · [Usage](USAGE.md) · [Plugins](PLUGINS.md) · [FAQ](FAQ.md) · [Benchmarks](BENCHMARKS.md) · [Design](DESIGN.md) · [Versioning](VERSIONING.md) · [Releasing](RELEASING.md)

How a change goes from a commit, through the CHANGELOG, to a release. What counts as a breaking change is in [VERSIONING.md](VERSIONING.md), and the commit format is in [CONTRIBUTING.md](../CONTRIBUTING.md).

## At a glance

```
commit (Conventional Commits) ──▶ PR: scripts/check.sh checks the commit format and CHANGELOG [Unreleased]
                                     │
                                     ▼  merged into main
release PR: node scripts/release.mjs bump <level>  ──▶ changes the version everywhere at once and
                                                       moves [Unreleased] to ## [X.Y.Z] - date
                                     │
                                     ▼  after the merge, the maintainer tags vX.Y.Z (release.mjs check --tag compares the tag with the version)
```

- **Commits are the source of the change history.** Every commit uses the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) format (checked by `scripts/check.sh` and `.githooks/commit-msg`), and its type decides the release level.
- **CHANGELOG.md is prose for people.** In the [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format, each user-visible change (`feat`, `fix`, `perf`, `revert`, a breaking change) gets one line under `## [Unreleased]` in the same PR. `node scripts/release.mjs notes` drafts it from the commits ([git-cliff](https://git-cliff.org), configured in `cliff.toml`).
- CHANGELOG entries are written in English, like the documentation, with a Korean copy of each entry in [ko/CHANGELOG.md](ko/CHANGELOG.md). The git-cliff draft comes from the commit messages; read it and rewrite it as sentences that make sense to users. Section names (`Added`, `Changed`, ...) and code and identifiers stay as they are in both files.
- **People write the sentences; machines check the structure.** For every PR, `scripts/check.sh` (`check-commits.mjs --changelog`) compares the sections of new entries in `[Unreleased]` with the sections of the git-cliff draft (see [Why it is semi-automatic](#why-it-is-semi-automatic) below).
- **The public interface is also checked by machine.** `api/c-abi.txt` (the `zeropty_*` symbols of `libzeropty_c.dylib` and the declarations in `zeropty.h`) and `api/node.txt` (the Node module exports) are the baselines. `scripts/check.sh` compares them with the build (`node scripts/check-api.mjs`), and when a baseline line disappears in a PR it requires a breaking mark (`!` or `BREAKING CHANGE:`) and a `**BREAKING**` entry ([VERSIONING.md](VERSIONING.md#machine-checks)).
- The tools in this repository **never commit, tag, push or publish.** They only change files; people check and do the rest.

## Why it is semi-automatic

We do not use the git-cliff output as the CHANGELOG verbatim; a person rewrites the draft for users. This is a deliberate choice.

- **The readers differ.** A commit message is a one-line note for people changing the code, and a CHANGELOG entry is documentation for people using zeropty. A list of commit titles leaves out "what you have to change".
- **Several commits become one entry.** When one feature is split over several `feat` commits, or a feature not released yet is fixed with a `fix` in the same cycle, the draft has several lines but users need one.
- **Instead, `scripts/check.sh` catches entries that are missing or filed in the wrong place.** `check-commits.mjs --changelog` classifies the PR's commits with the same rules as `cliff.toml` (`sectionsFor` in `scripts/lib/conventional.mjs`) and checks that the **new or changed entries** in `[Unreleased]` are in those sections.

| Commit | Section in the git-cliff draft | New entry the check requires |
|---|---|---|
| `feat` | Added | under `### Added` |
| `fix` | Fixed | under `### Fixed` |
| `fix(security)` | Security | under `### Security` |
| `perf`, `revert` | Changed | under `### Changed` |
| breaking change (`!`, `BREAKING CHANGE:`) | Changed | `- **BREAKING** ...` under `### Changed` or `### Removed` |
| `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style` | (left out) | none |
| any type + a `Changelog: skip` footer (except breaking changes) | (left out) | none |

- The only headings allowed in `[Unreleased]` are `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` and `Security`, in this order, each at most once. Every entry must be under a `###` heading. This is always checked, even with `ZEROPTY_SKIP_CHANGELOG=1`.
- The wording is not compared. The entries are rewritten rather than copied from the draft, so as long as the section matches, the check passes.
- If the two classification rules (`cliff.toml` and `sectionsFor`) disagree, `scripts/test/cliff.test.mjs` fails. It renders commits with the real git-cliff and compares the section of each line with `sectionsFor`.
- A commit that is not news to users, such as a `fix` for a feature not released yet (one still under `[Unreleased]`), gets a `Changelog: skip` footer at the end of its commit message. Only that commit is left out of the check and the git-cliff draft; the other commits in the same PR are still checked. If needed, reword the existing entry. It cannot be used for breaking changes (the commit check rejects it).
- If the whole PR is invisible to users but has `feat`/`fix` types, say so in the PR description, and the maintainer skips the entry check with `ZEROPTY_SKIP_CHANGELOG=1 scripts/check.sh`. The commit format is still checked.

## Where the version lives

`release.mjs bump` changes all of the following at once, and `tests/version.rs`, the `bindings/c` unit tests and `release.mjs check` verify that they agree.

| File | Contents |
|---|---|
| `Cargo.toml` | `[workspace.package] version`. The three crates inherit it with `version.workspace = true` |
| `Cargo.lock` | The `zeropty`, `zeropty-c` and `zeropty-node` entries |
| `bindings/c/include/zeropty.h` | `ZEROPTY_VERSION_MAJOR`, `MINOR`, `PATCH`, `PRERELEASE`, `IS_PRERELEASE`, `STRING` (`NUMBER` is computed) |
| `client/zeropty.mjs`, `client/embedded.mjs` | `export const VERSION` |
| `CHANGELOG.md` | `## [X.Y.Z] - YYYY-MM-DD` and the comparison links at the bottom |

`zeropty --version`, the server's `#{version}`, `zeropty_version()` and Node's `version()` come from the Cargo version at build time, so there is nothing else to change.

`bump` does not touch the Korean copy [ko/CHANGELOG.md](ko/CHANGELOG.md); when you polish the release section (step 4 below), apply the same heading and entries there by hand.

The links at the bottom of the CHANGELOG point **only to tags that exist**. If the previous version has a tag, they use `compare/vA...vB`; if not, they start from the commit where the `[Unreleased]` link starts (currently the 0.1.0 commit `ddd8811`). A link to a tag that does not exist makes `node --test scripts/test` fail.

## Commands

```sh
node scripts/release.mjs check              # whether the version agrees everywhere (also run by scripts/check.sh)
node scripts/release.mjs check --tag v0.2.0 # and whether the tag matches that version (also run by scripts/check.sh on a tagged HEAD)
node scripts/release.mjs next               # the next version, computed from the commits since the last tag
node scripts/release.mjs notes              # the [Unreleased] draft (git-cliff, fetched with npx if missing)
node scripts/release.mjs bump minor --dry-run
node scripts/release.mjs bump <major|minor|patch|auto|X.Y.Z[-pre]> [--date YYYY-MM-DD] [--generate]
node scripts/check-commits.mjs --range origin/main..HEAD --changelog   # only the commit and changelog check of scripts/check.sh (including the section comparison)
scripts/smoke-version.sh                    # whether the CLI, server, C and Node versions in dist/ agree (the version smoke tests step of scripts/check.sh)
rustup run 1.88.0 cargo check --workspace --all-targets --locked   # MSRV (the MSRV step of scripts/check.sh)
node scripts/check-api.mjs                  # whether the C ABI and Node exports in dist/ (./build.sh) match api/*.txt
node scripts/check-api.mjs --base origin/main   # whether lines removed from the baseline are marked as breaking (PR check)
node scripts/check-api.mjs --update         # update the baseline when you changed the interface on purpose (in the same commit)
node --test scripts/test/*.test.mjs         # tests for the tools themselves (including git-cliff rendering)
```

- `bump auto` uses the level `next` computes. Before 1.0, that is MINOR if there is a breaking change and PATCH otherwise.
- `bump` first checks that `check` passes and that the working tree is clean; if either fails, it changes no files.
- It stops if `[Unreleased]` is empty. With `--generate`, it fills it from the git-cliff draft (read and rewrite it anyway).
- After changing the version, check that `Cargo.lock` is consistent with `cargo metadata --locked`.

## Release procedure

1. **Check that `scripts/check.sh` passes on main.** Every step must pass without `--quick` (including MSRV, the examples and rustdoc).
2. **Decide the level.**
   ```sh
   git switch main && git pull
   node scripts/release.mjs next
   ```
   Compare the result with the rules in [VERSIONING.md](VERSIONING.md). If you raised the MSRV, or a `build` or `refactor` commit made a user-visible difference, the computation does not see it, so choose the level yourself.
3. **Bump the version on a release branch.**
   ```sh
   git switch -c release/0.2.0
   node scripts/release.mjs bump minor      # or auto, 0.2.0, 0.2.0-rc.1
   ```
4. **Polish the CHANGELOG.** Read the `## [0.2.0]` section from a user's point of view.
   - Merge lines about the same thing into one, and leave out internal details.
   - For a breaking change, write what users have to change, along with `**BREAKING**`.
   - If you mention performance numbers, use only values measured in [BENCHMARKS.md](BENCHMARKS.md).
   - Make the same changes in the Korean copy, [ko/CHANGELOG.md](ko/CHANGELOG.md).
5. **Verify.**
   ```sh
   cargo build --release --workspace && cargo test --release --workspace && ./build.sh
   dist/zeropty --version                   # zeropty 0.2.0
   node scripts/release.mjs check
   ```
6. **Commit and open a PR.** The commit message is `chore(release): 0.2.0` (git-cliff leaves this commit out of the CHANGELOG). Use the same PR title.
7. **After the merge, the maintainer creates the tag.**
   ```sh
   git switch main && git pull
   git tag -a v0.2.0 -m "zeropty 0.2.0"
   git push origin v0.2.0
   ```
   Before pushing, check that the tag matches the version with `node scripts/release.mjs check --tag v0.2.0` (on a tagged HEAD, `scripts/check.sh` runs this check too). If you create a GitHub Release, paste the CHANGELOG's `## [0.2.0]` section into its body as is.

> 0.1.0 is the state of the first public commit (`ddd8811`), and it has no tag yet. When the maintainer publishes it, the first tag goes **on that commit** (`git tag -a v0.1.0 ddd8811 -m "zeropty 0.1.0"`). Until then, `next` and `notes` compute from the beginning of the repository (the first commit is not in the Conventional Commits format, so it is left out), and the CHANGELOG links point to that commit. After tagging, you may change the `[Unreleased]` link to `compare/v0.1.0...HEAD`; the next `bump` changes it for you anyway.

## Formatting

The whole workspace is rustfmt-formatted (configured in `rustfmt.toml`). Once the branches in progress had all landed on main, it was formatted in one go with a single `style: cargo fmt --all` commit (`e12f84f`).

- The rustfmt step of `scripts/check.sh` checks the whole tree with `cargo fmt --all --check`. Run `cargo fmt --all` before committing.
- For an old branch that forked before the formatting commit, pick your side in the conflicting files and run `cargo fmt --all` again. Formatting is mechanical, so the result is the same.
- To skip the formatting commit in `git blame`, use `git blame --ignore-rev e12f84f <file>`.

## Prereleases and patch releases

- **Prerelease:** `node scripts/release.mjs bump 0.2.0-rc.1`. The tag is `v0.2.0-rc.1`. The final release is `bump patch` (`0.2.0-rc.1` → `0.2.0`) or `bump 0.2.0`. The header's `ZEROPTY_VERSION_PATCH` and `ZEROPTY_VERSION_NUMBER` hold only numbers, so `0.2.0-rc.1` and `0.2.0` are the same there. Tell them apart with `ZEROPTY_VERSION_PRERELEASE` (`"rc.1"`, `""` for a final release), `ZEROPTY_VERSION_IS_PRERELEASE` (1/0, usable in `#if`) and `ZEROPTY_VERSION_STRING` (`"0.2.0-rc.1"`).
- **Fixing an earlier version (backport):** create a branch from the tag (`git switch -c release/0.1.x v0.1.0`). Cherry-pick the fix commits, record them under `[Unreleased]`, and follow the same procedure with `bump patch`. Copy that version's section into the CHANGELOG on main as well.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `tests/version.rs` fails | A version was edited by hand. See where they disagree with `node scripts/release.mjs check`, and bring them back in line with `bump` |
| The commit messages step of `scripts/check.sh`: commit format | Fix the message with `git rebase -i` (`reword`). Fold fixup commits in with `--autosquash` |
| The commit messages step of `scripts/check.sh`: changelog | Add a line under `## [Unreleased]`. For a change users do not see, skip the entry check with `ZEROPTY_SKIP_CHANGELOG=1 scripts/check.sh` |
| `needs an entry under ### Fixed for: ...` | The entry is in another section. Move it to the section that matches the commit type (the table in [Why it is semi-automatic](#why-it-is-semi-automatic)). `node scripts/release.mjs notes` shows git-cliff's classification |
| `[Unreleased]: "### Fixes" is not one of ...` | Use only the six Keep a Changelog headings, in the order `Added` → `Security` |
| Fails because a breaking change has no `**BREAKING**` line | Add a `- **BREAKING** ...` line under `[Unreleased]` |
| `check-api.mjs`: `api/*.txt does not match the build` | If you changed the interface on purpose, run `node scripts/check-api.mjs --update` and commit `api/` with it. If not, revert the changed declarations or exports |
| `check-api.mjs --base`: `public API removed or changed` | A line disappeared from the baseline (a function removed, a signature changed). If intended, add `!` or `BREAKING CHANGE:` to the commit and `- **BREAKING** ...` under `[Unreleased]`; if not, add it under a new name instead |
| The rustfmt step of `scripts/check.sh` fails | Format with `cargo fmt --all` and commit ([Formatting](#formatting)) |
| The MSRV step of `scripts/check.sh`: `Rust 1.88.0 is not installed` | `rustup toolchain install 1.88.0` |
| `bump` stops with `nothing under ## [Unreleased]` | Write the entries, or pass `--generate` |
| A tag was created by mistake | Before pushing, `git tag -d vX.Y.Z`. After pushing, do not delete it; release the next PATCH instead |
