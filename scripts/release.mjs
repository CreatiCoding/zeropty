#!/usr/bin/env node
// zeropty's version, kept in one step everywhere it is written (docs/RELEASING.md):
//   Cargo.toml [workspace.package] (every crate inherits it), Cargo.lock, bindings/c/include/zeropty.h
//   (ZEROPTY_VERSION_*), client/zeropty.mjs and client/embedded.mjs (VERSION), and CHANGELOG.md.
// `zeropty --version`, zeropty_version() and the Node module's version() come from Cargo at build time.
//
//   node scripts/release.mjs check [--tag vX.Y.Z]   every place agrees (and with the tag, when given)
//   node scripts/release.mjs next                   the bump the commits since the last tag call for
//   node scripts/release.mjs notes                  [Unreleased] drafted from those commits (git-cliff)
//   node scripts/release.mjs release-notes [X.Y.Z]  the GitHub Release body, from CHANGELOG.md's ## [X.Y.Z]
//   node scripts/release.mjs bump <major|minor|patch|auto|X.Y.Z[-pre]> [--date YYYY-MM-DD] [--dry-run]
//                                 [--generate] [--allow-dirty]
//
// bump edits files only: it never commits, tags, pushes or publishes.
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { bumpFor, isMain, isMerge, parse } from './lib/conventional.mjs'

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
/** git-cliff used when none is on PATH (through npx); the only place it is pinned (cliff.toml). */
export const GIT_CLIFF = 'git-cliff@2.10.1'
const CRATES = ['zeropty', 'zeropty-c', 'zeropty-node']
const SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?$/

export function parseVersion(v) {
  const m = SEMVER.exec(v)
  if (!m) throw new Error(`not a SemVer version: ${v}`)
  return { major: +m[1], minor: +m[2], patch: +m[3], pre: m[4] ?? null }
}

/** -1, 0 or 1, by SemVer precedence (build metadata is not used here). */
export function compareVersions(a, b) {
  const x = parseVersion(a)
  const y = parseVersion(b)
  for (const k of ['major', 'minor', 'patch']) if (x[k] !== y[k]) return x[k] < y[k] ? -1 : 1
  if (x.pre === y.pre) return 0
  if (x.pre === null) return 1
  if (y.pre === null) return -1
  const p = x.pre.split('.')
  const q = y.pre.split('.')
  for (let i = 0; i < Math.max(p.length, q.length); i++) {
    if (p[i] === undefined) return -1
    if (q[i] === undefined) return 1
    if (p[i] === q[i]) continue
    const [n, m] = [/^\d+$/.test(p[i]), /^\d+$/.test(q[i])]
    if (n && m) return +p[i] < +q[i] ? -1 : 1
    if (n !== m) return n ? -1 : 1
    return p[i] < q[i] ? -1 : 1
  }
  return 0
}

/** The version after `current` for a bump name, or `bump` itself when it is a version. */
export function nextVersion(current, bump) {
  const v = parseVersion(current)
  switch (bump) {
    case 'major':
      return `${v.major + 1}.0.0`
    case 'minor':
      return `${v.major}.${v.minor + 1}.0`
    case 'patch':
      // A pre-release's patch bump releases it: 0.2.0-rc.1 -> 0.2.0.
      return v.pre ? `${v.major}.${v.minor}.${v.patch}` : `${v.major}.${v.minor}.${v.patch + 1}`
    default:
      parseVersion(bump)
      return bump
  }
}

// ---------------------------------------------------------------- the files

const files = (root) => ({
  cargo: path.join(root, 'Cargo.toml'),
  lock: path.join(root, 'Cargo.lock'),
  header: path.join(root, 'bindings/c/include/zeropty.h'),
  clients: [path.join(root, 'client/zeropty.mjs'), path.join(root, 'client/embedded.mjs')],
  changelog: path.join(root, 'CHANGELOG.md'),
})

const WS_VERSION = /(\[workspace\.package\](?:(?!\n\[)[\s\S])*?\nversion = ")([^"]+)(")/
const lockEntry = (name) => new RegExp(`(\\nname = "${name}"\\nversion = ")([^"]+)(")`)
const define = (name) => new RegExp(`(\\n#define ${name} )(\\S+)()`)
const CLIENT_VERSION = /(\nexport const VERSION = ')([^']+)(')/

/** Replaces the one match of `re`'s second group (of three) with `value`; throws when `re` does not match. */
function put(text, re, value, where) {
  if (!re.test(text)) throw new Error(`${where}: ${re} not found`)
  return text.replace(re, (_, a, _old, c) => `${a}${value}${c}`)
}

/** Every version written in the tree, by where it is: { 'Cargo.toml': '0.1.0', ... }. */
export function versions(root = ROOT) {
  const f = files(root)
  const out = {}
  const grab = (file, re, label) => {
    const m = re.exec(readFileSync(file, 'utf8'))
    out[label] = m ? m[2] : null
  }
  grab(f.cargo, WS_VERSION, 'Cargo.toml [workspace.package]')
  for (const c of CRATES) grab(f.lock, lockEntry(c), `Cargo.lock ${c}`)
  const h = readFileSync(f.header, 'utf8')
  const num = (n) => define(n).exec(h)?.[2] ?? null
  out['zeropty.h ZEROPTY_VERSION_STRING'] = (define('ZEROPTY_VERSION_STRING').exec(h)?.[2] ?? '').replace(/"/g, '') || null
  out['zeropty.h ZEROPTY_VERSION_MAJOR.MINOR.PATCH'] = ['MAJOR', 'MINOR', 'PATCH'].map((k) => num(`ZEROPTY_VERSION_${k}`)).join('.')
  // Both pre-release macros, as the version they say: "0.2.0-rc.1" (or "0.2.0") when they agree with each other.
  const pre = num('ZEROPTY_VERSION_PRERELEASE')?.replace(/"/g, '')
  const isPre = num('ZEROPTY_VERSION_IS_PRERELEASE')
  out['zeropty.h ZEROPTY_VERSION_PRERELEASE'] =
    pre === undefined || isPre !== (pre ? '1' : '0') ? `${pre} (IS_PRERELEASE ${isPre})` : `${out['zeropty.h ZEROPTY_VERSION_MAJOR.MINOR.PATCH']}${pre ? `-${pre}` : ''}`
  for (const c of f.clients) grab(c, CLIENT_VERSION, path.relative(root, c))
  return out
}

/** Problems with the tree's versions: they disagree, the header's numbers miss, CHANGELOG.md lacks the version. */
export function check(root = ROOT, tag = null) {
  const found = versions(root)
  const version = found['Cargo.toml [workspace.package]']
  const problems = []
  if (!version) return ['Cargo.toml: no version in [workspace.package]']
  let core = null
  try {
    const v = parseVersion(version)
    core = `${v.major}.${v.minor}.${v.patch}`
  } catch (e) {
    problems.push(e.message)
  }
  for (const [where, v] of Object.entries(found)) {
    const want = where.endsWith('MAJOR.MINOR.PATCH') ? core : version
    if (v !== want) problems.push(`${where} is ${v}, not ${want}`)
  }
  for (const m of ['bindings/c/Cargo.toml', 'bindings/node/Cargo.toml', 'Cargo.toml']) {
    const pkg = readFileSync(path.join(root, m), 'utf8').split('[package]')[1]?.split('\n[')[0] ?? ''
    if (!pkg.includes('\nversion.workspace = true')) problems.push(`${m}: [package] must say version.workspace = true`)
  }
  const log = readFileSync(files(root).changelog, 'utf8')
  if (!/^## \[Unreleased\]$/m.test(log)) problems.push('CHANGELOG.md: no ## [Unreleased]')
  if (!new RegExp(`^## \\[${escape(version)}\\] - \\d{4}-\\d{2}-\\d{2}$`, 'm').test(log)) problems.push(`CHANGELOG.md: no ## [${version}] - YYYY-MM-DD`)
  if (!new RegExp(`^\\[${escape(version)}\\]: \\S+`, 'm').test(log)) problems.push(`CHANGELOG.md: no [${version}]: link`)
  if (tag !== null && tag !== `v${version}`) problems.push(`tag ${tag} is not v${version}`)
  return problems
}

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** Moves [Unreleased]'s entries under a new `## [version] - date` and updates the links at the bottom. */
export function releaseChangelog(text, version, date, previous) {
  const m = /^## \[Unreleased\][^\n]*\n([\s\S]*?)(?=^## \[|^\[[^\]]+\]: |(?![\s\S]))/m.exec(text)
  if (!m) throw new Error('CHANGELOG.md: no ## [Unreleased]')
  const body = m[1].trim()
  if (!body) throw new Error('CHANGELOG.md: nothing under ## [Unreleased] (write the entries, or pass --generate)')
  if (new RegExp(`^## \\[${escape(version)}\\]`, 'm').test(text)) throw new Error(`CHANGELOG.md: ${version} is already there`)
  const section = `## [Unreleased]\n\n## [${version}] - ${date}\n\n${body}\n\n`
  let out = text.slice(0, m.index) + section + text.slice(m.index + m[0].length)
  const link = /^\[Unreleased\]: (\S+)\/compare\/(\S+)\.\.\.HEAD$/m.exec(out)
  if (!link) throw new Error('CHANGELOG.md: no "[Unreleased]: <repo>/compare/<tag>...HEAD" link')
  const [line, repo, from] = link
  const prevTag = previous ? `v${previous}` : from
  out = out.replace(line, `[Unreleased]: ${repo}/compare/v${version}...HEAD\n[${version}]: ${repo}/compare/${prevTag}...v${version}`)
  return out
}

/**
 * The GitHub Release body for `version`: its CHANGELOG section with each wrapped entry and paragraph on one
 * line (a release body keeps line breaks; no code blocks or tables there to keep), relative links made absolute at the tag (a release body resolves them
 * under /releases/, where they break), and links to the full diff and the CHANGELOGs.
 */
export function releaseNotes(text, version) {
  const m = new RegExp(`^## \\[${escape(version)}\\][^\\n]*\\n([\\s\\S]*?)(?=^## \\[|^\\[[^\\]]+\\]: |(?![\\s\\S]))`, 'm').exec(text)
  if (!m) throw new Error(`CHANGELOG.md: no ## [${version}]`)
  const l = new RegExp(`^\\[${escape(version)}\\]: ((\\S+)/(?:compare|releases/tag)/\\S+)$`, 'm').exec(text)
  if (!l) throw new Error(`CHANGELOG.md: no [${version}] link`)
  const [, link, repo] = l
  const blob = `${repo}/blob/v${version}`
  const body = m[1]
    .trim()
    // Not a blank line, a heading or another entry: the line continues the one before it.
    .replace(/(\S)\n(?!\n|#|- ) */g, '$1 ')
    .replace(/\]\((?![a-z]+:|#)([^)]+)\)/g, `](${blob}/$1)`)
  return `${body}\n\n---\n\n**Full changes:** ${link}\n\n[CHANGELOG](${blob}/CHANGELOG.md) · [한국어](${blob}/docs/ko/CHANGELOG.md) · [Versioning](${blob}/docs/VERSIONING.md)\n`
}

/** Writes `version` everywhere (CHANGELOG.md too, unless changelog is false). Returns the files changed. */
export function setVersion(root, version, { date, changelog = true } = {}) {
  const v = parseVersion(version)
  const f = files(root)
  const previous = versions(root)['Cargo.toml [workspace.package]']
  const edits = new Map()
  const edit = (file, fn) => edits.set(file, fn(edits.get(file) ?? readFileSync(file, 'utf8')))
  edit(f.cargo, (t) => put(t, WS_VERSION, version, 'Cargo.toml'))
  for (const c of CRATES) edit(f.lock, (t) => put(t, lockEntry(c), version, `Cargo.lock ${c}`))
  edit(f.header, (t) => {
    t = put(t, define('ZEROPTY_VERSION_MAJOR'), String(v.major), 'zeropty.h')
    t = put(t, define('ZEROPTY_VERSION_MINOR'), String(v.minor), 'zeropty.h')
    t = put(t, define('ZEROPTY_VERSION_PATCH'), String(v.patch), 'zeropty.h')
    t = put(t, define('ZEROPTY_VERSION_PRERELEASE'), `"${v.pre ?? ''}"`, 'zeropty.h')
    t = put(t, define('ZEROPTY_VERSION_IS_PRERELEASE'), v.pre ? '1' : '0', 'zeropty.h')
    return put(t, define('ZEROPTY_VERSION_STRING'), `"${version}"`, 'zeropty.h')
  })
  for (const c of f.clients) edit(c, (t) => put(t, CLIENT_VERSION, version, path.relative(root, c)))
  // The compare link starts at the previous version's tag; when that has no tag, at whatever the
  // [Unreleased] link starts from, a commit, so no link points at a missing tag.
  if (changelog) edit(f.changelog, (t) => releaseChangelog(t, version, date, hasTag(root, `v${previous}`) ? previous : null))
  // Every edit is computed before any file is written: a failure leaves the tree as it was.
  for (const [file, text] of edits) writeFileSync(file, text)
  return [...edits.keys()].map((p) => path.relative(root, p))
}

// ---------------------------------------------------------------- git

const git = (root, ...args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'] }).trim()

/** Whether `tag` exists in the repository at `root` (false outside a repository). */
export function hasTag(root, tag) {
  try {
    return git(root, 'tag', '--list', tag) === tag
  } catch {
    return false
  }
}

/** The newest vX.Y.Z tag reachable from HEAD, or null. */
export function lastTag(root = ROOT) {
  try {
    return git(root, 'describe', '--tags', '--abbrev=0', '--match', 'v[0-9]*')
  } catch {
    return null
  }
}

/** Parsed commits since the last tag (all of them with no tag), merges left out. */
export function commitsSince(root = ROOT, tag = lastTag(root)) {
  const out = git(root, 'log', '--no-merges', '--format=%B%x01', ...(tag ? [`${tag}..HEAD`] : ['HEAD']))
  return out
    .split('\x01')
    .map((s) => s.trim())
    .filter((s) => s && !isMerge(s))
    .map((message) => ({ message, ...parse(message) }))
}

/** git-cliff's [Unreleased] draft (or `## [version] - today` with tag), sections only. */
export function cliff(root = ROOT, tag = null) {
  const args = ['--config', path.join(root, 'cliff.toml'), '--unreleased', '--strip', 'all', ...(tag ? ['--tag', tag] : [])]
  const run = (cmd, pre) => execFileSync(cmd, [...pre, ...args], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
  try {
    return run('git-cliff', [])
  } catch (e) {
    if (e.code !== 'ENOENT') throw e
    return run('npx', ['--yes', GIT_CLIFF])
  }
}

// ---------------------------------------------------------------- commands

function main(argv) {
  const [cmd, ...rest] = argv
  const flag = (name) => {
    const i = rest.indexOf(name)
    return i < 0 ? null : rest.splice(i, 2)[1] ?? ''
  }
  const has = (name) => {
    const i = rest.indexOf(name)
    if (i >= 0) rest.splice(i, 1)
    return i >= 0
  }
  const current = versions(ROOT)['Cargo.toml [workspace.package]']

  if (cmd === 'check') {
    const problems = check(ROOT, flag('--tag'))
    for (const p of problems) console.error(`✗ ${p}`)
    if (!problems.length) console.log(`versions agree: ${current}`)
    return problems.length ? 1 : 0
  }

  if (cmd === 'next') {
    const tag = lastTag()
    const commits = commitsSince(ROOT, tag)
    const bump = bumpFor(commits, parseVersion(current).major)
    console.log(`current ${current}, last tag ${tag ?? '(none)'}, ${commits.length} commit(s) since`)
    for (const c of commits.filter((c) => !c.ok)) console.log(`  not Conventional (ignored): ${c.message.split('\n')[0]}`)
    if (!bump) console.log('nothing user-visible: no release needed')
    else console.log(`${bump} -> ${nextVersion(current, bump)}`)
    return 0
  }

  if (cmd === 'notes') {
    process.stdout.write(cliff(ROOT))
    return 0
  }

  if (cmd === 'release-notes') {
    process.stdout.write(releaseNotes(readFileSync(files(ROOT).changelog, 'utf8'), rest[0] ?? current))
    return 0
  }

  if (cmd === 'bump') {
    const dry = has('--dry-run')
    const generate = has('--generate')
    const dirtyOk = has('--allow-dirty')
    const date = flag('--date') ?? new Date().toISOString().slice(0, 10)
    const [what] = rest
    if (!what || !/^\d{4}-\d{2}-\d{2}$/.test(date)) {
      console.error('usage: release.mjs bump <major|minor|patch|auto|X.Y.Z[-pre]> [--date YYYY-MM-DD] [--dry-run] [--generate] [--allow-dirty]')
      return 2
    }
    const problems = check(ROOT)
    if (problems.length) {
      for (const p of problems) console.error(`✗ ${p}`)
      console.error('fix these first: the tree must agree on one version before it moves to the next')
      return 1
    }
    let bump = what
    if (what === 'auto') {
      bump = bumpFor(commitsSince(), parseVersion(current).major)
      if (!bump) {
        console.error('no user-visible commits since the last tag: nothing to release')
        return 1
      }
    }
    const version = nextVersion(current, bump)
    if (compareVersions(version, current) <= 0) {
      console.error(`${version} is not after ${current}`)
      return 1
    }
    if (!dry && !dirtyOk && git(ROOT, 'status', '--porcelain')) {
      console.error('the working tree has changes: commit or stash them (or pass --allow-dirty)')
      return 1
    }
    console.log(`${current} -> ${version} (${bump}), dated ${date}`)
    if (dry) return 0
    if (generate) {
      const log = readFileSync(files(ROOT).changelog, 'utf8')
      const m = /^## \[Unreleased\]\n([\s\S]*?)(?=^## \[|^\[[^\]]+\]: )/m.exec(log)
      if (m && !m[1].trim()) {
        const draft = cliff(ROOT).replace(/^## \[Unreleased\]\n/m, '').trim()
        if (draft) writeFileSync(files(ROOT).changelog, log.replace(m[0], `## [Unreleased]\n\n${draft}\n\n`))
      }
    }
    const changed = setVersion(ROOT, version, { date })
    // Cargo agrees with the edited lockfile (fails if the edit missed something).
    execFileSync('cargo', ['metadata', '--format-version', '1', '--locked'], { cwd: ROOT, stdio: ['ignore', 'ignore', 'inherit'] })
    for (const p of changed) console.log(`  updated ${p}`)
    console.log(`
next (by hand; this script never commits, tags or pushes):
  review CHANGELOG.md's ## [${version}] and edit it for readers
  cargo test --release --workspace && ./build.sh
  git commit -am "chore(release): ${version}"
  then follow docs/RELEASING.md for the pull request and the tag`)
    return 0
  }

  console.error('usage: release.mjs check [--tag vX.Y.Z] | next | notes | release-notes [X.Y.Z] | bump <major|minor|patch|auto|X.Y.Z> [...]')
  return 2
}

if (isMain(import.meta.url)) {
  try {
    process.exit(main(process.argv.slice(2)))
  } catch (e) {
    console.error(`release: ${e.message}`)
    process.exit(1)
  }
}
