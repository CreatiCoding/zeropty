// node --test scripts/test: the release and commit tooling, on copies of the real files.
import assert from 'node:assert/strict'
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, describe, test } from 'node:test'
import { bumpFor, parse, sectionsFor, userVisible } from '../lib/conventional.mjs'
import { changelogProblems, sections, unreleased } from '../check-commits.mjs'
import { ROOT, check, compareVersions, hasTag, nextVersion, releaseChangelog, setVersion, versions } from '../release.mjs'

const COPIED = ['Cargo.toml', 'Cargo.lock', 'bindings/c/Cargo.toml', 'bindings/node/Cargo.toml', 'bindings/c/include/zeropty.h', 'client/zeropty.mjs', 'client/embedded.mjs', 'CHANGELOG.md']
const tmp = []
function copy() {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'zeropty-release-'))
  tmp.push(dir)
  for (const f of COPIED) cpSync(path.join(ROOT, f), path.join(dir, f), { recursive: true })
  return dir
}
after(() => {
  for (const d of tmp) rmSync(d, { recursive: true, force: true })
})

describe('conventional commits', () => {
  test('valid headers', () => {
    for (const m of ['feat: add x', 'fix(term): keep cursor', 'perf(engine)!: pack rows', 'docs(usage): 한국어 설명', 'feat(c-abi): add zeropty_version()\n\nbody\n\nRefs: #1']) {
      assert.ok(parse(m).ok, `${m}: ${parse(m).errors}`)
    }
  })
  test('invalid headers', () => {
    for (const m of ['Add x', 'feature: add x', 'feat:add x', 'feat(Term): x', 'fix: x.', `feat: ${'x'.repeat(100)}`, 'fix: x\nbody without blank line']) {
      assert.ok(!parse(m).ok, m)
    }
  })
  test('breaking: ! or footer', () => {
    assert.ok(parse('refactor!: drop -X').breaking)
    assert.ok(parse('feat: x\n\nBREAKING CHANGE: -X is gone').breaking)
    assert.ok(parse('feat: x\n\nBREAKING-CHANGE: -X is gone').breaking)
    assert.ok(!parse('feat: x\n\nnot a BREAKING CHANGE: mention').breaking)
  })
  test('user-visible types', () => {
    assert.ok(userVisible(parse('feat: x')))
    assert.ok(userVisible(parse('perf: x')))
    assert.ok(!userVisible(parse('refactor: x')))
    assert.ok(userVisible(parse('refactor!: x')))
    assert.ok(!userVisible(parse('docs: x')))
  })
  test('bump before and after 1.0', () => {
    const c = (...ms) => ms.map(parse)
    assert.equal(bumpFor(c('docs: x', 'ci: y'), 0), null)
    assert.equal(bumpFor(c('fix: x'), 0), 'patch')
    assert.equal(bumpFor(c('feat: x'), 0), 'patch')
    assert.equal(bumpFor(c('feat: x', 'fix!: y'), 0), 'minor')
    assert.equal(bumpFor(c('fix: x'), 1), 'patch')
    assert.equal(bumpFor(c('feat: x', 'fix: y'), 1), 'minor')
    assert.equal(bumpFor(c('feat: x', 'refactor!: y'), 1), 'major')
  })
})

describe('versions', () => {
  test('next and compare', () => {
    assert.equal(nextVersion('0.1.0', 'patch'), '0.1.1')
    assert.equal(nextVersion('0.1.3', 'minor'), '0.2.0')
    assert.equal(nextVersion('0.9.1', 'major'), '1.0.0')
    assert.equal(nextVersion('0.2.0-rc.1', 'patch'), '0.2.0')
    assert.equal(nextVersion('0.1.0', '0.3.0-rc.1'), '0.3.0-rc.1')
    assert.throws(() => nextVersion('0.1.0', '1.0'))
    assert.equal(compareVersions('0.2.0', '0.10.0'), -1)
    assert.equal(compareVersions('1.0.0-rc.2', '1.0.0-rc.10'), -1)
    assert.equal(compareVersions('1.0.0-rc.1', '1.0.0'), -1)
    assert.equal(compareVersions('1.0.0-alpha', '1.0.0-1'), 1)
  })

  test('the repository agrees with itself', () => {
    assert.deepEqual(check(ROOT), [])
  })

  test('setVersion moves every place, and check sees a missed one', () => {
    const dir = copy()
    // An entry to release, whatever the repository's own [Unreleased] holds (right after a release it is empty).
    const log = path.join(dir, 'CHANGELOG.md')
    writeFileSync(log, readFileSync(log, 'utf8').replace('## [Unreleased]\n', '## [Unreleased]\n\n### Fixed\n\n- a fix\n'))
    const changed = setVersion(dir, '0.2.0-rc.1', { date: '2027-01-02' })
    assert.deepEqual(changed.sort(), ['CHANGELOG.md', 'Cargo.lock', 'Cargo.toml', 'bindings/c/include/zeropty.h', 'client/embedded.mjs', 'client/zeropty.mjs'])
    assert.deepEqual(check(dir), [])
    assert.deepEqual(check(dir, 'v0.2.0-rc.1'), [])
    assert.equal(check(dir, 'v0.2.0').length, 1)
    for (const v of Object.values(versions(dir))) assert.match(v, /^0\.2\.0(-rc\.1)?$/)
    const h = readFileSync(path.join(dir, 'bindings/c/include/zeropty.h'), 'utf8')
    assert.match(h, /#define ZEROPTY_VERSION_MINOR 2\n/)
    assert.match(h, /#define ZEROPTY_VERSION_STRING "0\.2\.0-rc\.1"\n/)
    assert.match(h, /#define ZEROPTY_VERSION_PRERELEASE "rc\.1"\n/)
    assert.match(h, /#define ZEROPTY_VERSION_IS_PRERELEASE 1\n/)
    // The release after it clears both again, and a hand edit of one alone is caught.
    setVersion(dir, '0.2.0', { date: '2027-01-03', changelog: false })
    const released = readFileSync(path.join(dir, 'bindings/c/include/zeropty.h'), 'utf8')
    assert.match(released, /#define ZEROPTY_VERSION_PRERELEASE ""\n/)
    assert.match(released, /#define ZEROPTY_VERSION_IS_PRERELEASE 0\n/)
    writeFileSync(path.join(dir, 'bindings/c/include/zeropty.h'), released.replace('IS_PRERELEASE 0', 'IS_PRERELEASE 1'))
    assert.equal(check(dir).filter((p) => p.includes('PRERELEASE')).length, 1)
    setVersion(dir, '0.2.0-rc.1', { date: '2027-01-02', changelog: false })
    // Other crates in the lockfile are left alone.
    const before = readFileSync(path.join(ROOT, 'Cargo.lock'), 'utf8').split('\n').filter((l) => l.startsWith('version = ')).length
    const lock = readFileSync(path.join(dir, 'Cargo.lock'), 'utf8')
    assert.equal(lock.split('\n').filter((l) => l.startsWith('version = ')).length, before)
    assert.equal(lock.split('version = "0.2.0-rc.1"').length - 1, 3)

    const js = path.join(dir, 'client/zeropty.mjs')
    writeFileSync(js, readFileSync(js, 'utf8').replace("VERSION = '0.2.0-rc.1'", "VERSION = '0.1.0'"))
    assert.deepEqual(check(dir), ['client/zeropty.mjs is 0.1.0, not 0.2.0-rc.1'])
  })

  test('a failed bump writes nothing', () => {
    const dir = copy()
    const log = path.join(dir, 'CHANGELOG.md')
    writeFileSync(log, readFileSync(log, 'utf8').replace(/## \[Unreleased\]\n[\s\S]*?(?=## \[0\.1\.0\])/, '## [Unreleased]\n\n'))
    const cargo = readFileSync(path.join(dir, 'Cargo.toml'), 'utf8')
    assert.throws(() => setVersion(dir, '0.1.1', { date: '2027-01-02' }), /nothing under/)
    assert.equal(readFileSync(path.join(dir, 'Cargo.toml'), 'utf8'), cargo)
  })
})

describe('changelog', () => {
  const log = `# Changelog

## [Unreleased]

### Fixed

- a fix

## [0.1.0] - 2026-10-20

### Added

- first

[Unreleased]: https://github.com/o/r/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/o/r/releases/tag/v0.1.0
`
  test('release moves [Unreleased] and the links', () => {
    const out = releaseChangelog(log, '0.1.1', '2026-11-01', '0.1.0')
    assert.equal(
      out,
      `# Changelog

## [Unreleased]

## [0.1.1] - 2026-11-01

### Fixed

- a fix

## [0.1.0] - 2026-10-20

### Added

- first

[Unreleased]: https://github.com/o/r/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/o/r/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/o/r/releases/tag/v0.1.0
`,
    )
    assert.throws(() => releaseChangelog(out, '0.1.2', '2026-11-02', '0.1.1'), /nothing under/)
    assert.throws(() => releaseChangelog(log, '0.1.0', '2026-11-02', '0.1.0'), /already there/)
  })
  test('before the first tag, links start at the commit the [Unreleased] link names', () => {
    const sha = 'ddd8811b1cff59992d96ddc5cad522e1a0568254'
    const untagged = log
      .replace('compare/v0.1.0...HEAD', `compare/${sha}...HEAD`)
      .replace('releases/tag/v0.1.0', `tree/${sha}`)
    const out = releaseChangelog(untagged, '0.1.1', '2026-11-01', null)
    assert.match(out, /^\[Unreleased\]: https:\/\/github\.com\/o\/r\/compare\/v0\.1\.1\.\.\.HEAD$/m)
    assert.match(out, new RegExp(`^\\[0\\.1\\.1\\]: https://github\\.com/o/r/compare/${sha}\\.\\.\\.v0\\.1\\.1$`, 'm'))
    assert.match(out, new RegExp(`^\\[0\\.1\\.0\\]: https://github\\.com/o/r/tree/${sha}$`, 'm'))
  })
  test('the repository links no tag that does not exist', () => {
    const text = readFileSync(path.join(ROOT, 'CHANGELOG.md'), 'utf8')
    // Except the version being released: its release PR links it, and the tag comes after the merge.
    const releasing = `v${versions(ROOT)['Cargo.toml [workspace.package]']}`
    for (const [, tag] of text.matchAll(/^\[[^\]]+\]: \S+?(?:\/tag\/|\/compare\/|\.\.\.)(v\d[^.\s]*\.\d+\.\d+[^.\s]*)/gm)) {
      assert.ok(tag === releasing || hasTag(ROOT, tag), `CHANGELOG.md links ${tag}, which is not a tag`)
    }
  })
  test('[Unreleased] section text', () => {
    assert.equal(unreleased(log), '### Fixed\n\n- a fix')
    assert.equal(unreleased('# x\n\n## [Unreleased]\n\n- only\n'), '- only')
    assert.equal(unreleased(null), '')
  })
})

describe('[Unreleased] against the commits (check-commits --changelog)', () => {
  const log = (body) => `# Changelog\n\n## [Unreleased]\n\n${body}\n\n## [0.1.0] - 2026-10-01\n\n### Added\n\n- first\n`
  const base = log('### Added\n\n- `--version`')
  const commits = (...m) => m.map(parse)

  test('sections: entries with continuation lines, Keep a Changelog headings in order', () => {
    const { sections: s, errors } = sections('### Added\n\n- one\n  more\n- two\n\n### Fixed\n\n- three')
    assert.deepEqual(errors, [])
    assert.deepEqual([...s], [['Added', ['one more', 'two']], ['Fixed', ['three']]])
    assert.match(sections('### Fixes\n\n- x').errors[0], /not one of Added/)
    assert.match(sections('### Fixed\n\n- x\n\n### Added\n\n- y').errors[0], /comes after "### Fixed"/)
    assert.match(sections('### Added\n\n- x\n\n### Added\n\n- y').errors[0], /twice/)
    assert.match(sections('- loose').errors[0], /outside a ### section/)
  })
  test('sectionsFor follows cliff.toml: feat, fix, fix(security), perf, revert, breaking, hidden types', () => {
    const at = (m) => sectionsFor(parse(m))
    assert.deepEqual(at('feat(cli): x'), ['Added'])
    assert.deepEqual(at('fix(term): x'), ['Fixed'])
    assert.deepEqual(at('fix(security): x'), ['Security'])
    assert.deepEqual(at('perf: x'), ['Changed'])
    assert.deepEqual(at('revert: x'), ['Changed'])
    assert.deepEqual(at('fix(security)!: x'), ['Changed', 'Removed'])
    assert.deepEqual(at('docs: x\n\nBREAKING CHANGE: y'), ['Changed', 'Removed'])
    assert.equal(at('refactor: x'), null)
    assert.equal(at('fix(cli): x\n\nChangelog: skip'), null)
    assert.ok(!parse('feat!: x\n\nChangelog: skip').ok)
  })
  test('each user-visible commit needs a new entry in its own section', () => {
    const fixed = log('### Added\n\n- `--version`\n\n### Fixed\n\n- 커서 위치')
    assert.deepEqual(changelogProblems(base, fixed, commits('fix(term): keep the cursor')), [])
    // An entry under the wrong section (the draft files a fix under Fixed).
    const wrong = log('### Added\n\n- `--version`\n- 커서 위치')
    assert.deepEqual(changelogProblems(base, wrong, commits('fix(term): keep the cursor')), ['needs an entry under ### Fixed for: fix(term): keep the cursor'])
    // A feat and a fix need both sections; a docs commit needs none.
    const both = commits('feat(cli): add -V', 'fix(term): keep the cursor', 'docs: typo')
    assert.deepEqual(changelogProblems(base, fixed, both), ['needs an entry under ### Added for: feat(cli): add -V'])
    assert.deepEqual(changelogProblems(base, log('### Added\n\n- `--version`\n- `-V`\n\n### Fixed\n\n- 커서'), both), [])
    assert.deepEqual(changelogProblems(base, base, commits('docs: typo')), [])
    // A fix to a feature still under [Unreleased] can say so instead of adding an entry.
    assert.deepEqual(changelogProblems(base, base, commits('fix(cli): keep -V\n\nChangelog: skip')), [])
    assert.match(changelogProblems(base, base, commits('fix: x'))[0], /did not change/)
    assert.deepEqual(changelogProblems(base, log('### Added\n\n- `--version`\n\n### Security\n\n- 권한'), commits('fix(security): refuse others')), [])
  })
  test('a breaking commit needs a **BREAKING** entry under Changed or Removed', () => {
    const c = commits('feat(proto)!: rename the reply field')
    assert.match(changelogProblems(base, log('### Added\n\n- `--version`\n- 응답 필드 이름'), c)[0], /BREAKING/)
    assert.match(changelogProblems(base, log('### Added\n\n- `--version`\n\n### Changed\n\n- 응답 필드 이름'), c)[0], /BREAKING/)
    assert.deepEqual(changelogProblems(base, log('### Added\n\n- `--version`\n\n### Changed\n\n- **BREAKING** 응답 필드 `code` → `status`'), c), [])
    assert.deepEqual(changelogProblems(base, log('### Added\n\n- `--version`\n\n### Removed\n\n- **BREAKING** `-X` 플래그'), commits('refactor!: drop -X')), [])
  })
  test("the repository's own [Unreleased] is well-formed", () => {
    assert.deepEqual(sections(unreleased(readFileSync(path.join(ROOT, 'CHANGELOG.md'), 'utf8'))).errors, [])
  })
})
