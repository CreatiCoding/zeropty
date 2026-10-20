// node --test scripts/test: cliff.toml renders Conventional Commits as CHANGELOG.md sections, run through the
// pinned git-cliff (release.mjs GIT_CLIFF, through npx when none is on PATH) in a scratch repository.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdtempSync, rmSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, test } from 'node:test'
import { ROOT, cliff } from '../release.mjs'
import { parse, sectionsFor } from '../lib/conventional.mjs'

// GIT_DIR and friends (a git hook exports them) would point git and git-cliff at the repository the tests
// run from, not the scratch one: the commits and config below would land there.
for (const name of execFileSync('git', ['rev-parse', '--local-env-vars'], { encoding: 'utf8' }).split('\n')) {
  if (name) delete process.env[name]
}

const dir = mkdtempSync(path.join(os.tmpdir(), 'zeropty-cliff-'))
after(() => rmSync(dir, { recursive: true, force: true }))

const git = (...args) => execFileSync('git', ['-C', dir, ...args], { stdio: ['ignore', 'pipe', 'pipe'] })
git('init', '-q')
git('config', 'user.email', 'test@example.com')
git('config', 'user.name', 'test')
git('config', 'commit.gpgsign', 'false')
copyFileSync(path.join(ROOT, 'cliff.toml'), path.join(dir, 'cliff.toml'))
const messages = []
const commit = (message) => {
  messages.push(message)
  git('commit', '-q', '--allow-empty', '--no-verify', '-m', message)
}

commit('zeropty: not a Conventional Commit')
commit('feat(cli): add --version')
commit('fix(term): keep the cursor\n\nA body that explains it.\n\nBREAKING CHANGE: capture-pane -J joins wrapped rows')
commit('feat(proto)!: rename the reply field\n\nBody.\n\nBREAKING CHANGE: replies say "status",\nnot "code"\nRefs: #12')
commit('refactor(engine)!: drop the -X flag')
commit('fix(c-abi): free with zeropty_free\n\nBREAKING-CHANGE: zeropty_release is gone')
commit('perf(term): pack rows\n\nOnly a body.')
commit('fix(security): refuse other users')
commit('docs(usage): not in the changelog')
commit('fix(cli): exit 1 again for display with no session\n\nThe #{version} feature is unreleased.\n\nChangelog: skip')
commit('chore(release): 0.1.1')

test('cliff.toml: groups, scopes and BREAKING footers', () => {
  const out = cliff(dir)
  assert.equal(
    out.trim(),
    `## [Unreleased]

### Added

- cli: Add --version

### Changed

- **BREAKING** term: Keep the cursor: capture-pane -J joins wrapped rows
- **BREAKING** proto: Rename the reply field: replies say "status", not "code"
- **BREAKING** engine: Drop the -X flag
- **BREAKING** c-abi: Free with zeropty_free: zeropty_release is gone
- term: Pack rows

### Security

- Refuse other users`,
  )
})

// check-commits --changelog compares CHANGELOG.md's sections with sectionsFor(); this keeps that mapping and
// cliff.toml's commit_parsers the same, so `check-commits.mjs --changelog` and `release.mjs notes` never disagree.
test('cliff.toml files each commit where sectionsFor() says', () => {
  const rendered = []
  let group = null
  for (const line of cliff(dir).split('\n')) {
    if (line.startsWith('### ')) group = line.slice(4).trim()
    else if (line.startsWith('- ')) rendered.push({ line, group })
  }
  const visible = messages.map(parse).filter((c) => sectionsFor(c))
  assert.equal(rendered.length, visible.length)
  for (const c of visible) {
    const subject = c.subject[0].toUpperCase() + c.subject.slice(1)
    const hit = rendered.filter((r) => r.line.includes(subject))
    assert.equal(hit.length, 1, `${c.subject}: rendered once`)
    assert.ok(sectionsFor(c).includes(hit[0].group), `${c.type}: git-cliff says ${hit[0].group}, sectionsFor ${sectionsFor(c)}`)
  }
})
