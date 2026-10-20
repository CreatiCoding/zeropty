#!/usr/bin/env node
// The compatibility surface of docs/VERSIONING.md (sections 4 and 5), checked by machine: the C ABI (the
// zeropty_* symbols libzeropty_c.dylib exports and the declarations in zeropty.h) and the Node API (what
// zeropty.node, client/zeropty.mjs and client/embedded.mjs export) are listed in api/*.txt, and
//
//   node scripts/check-api.mjs                     the build in dist/ (./build.sh) matches api/*.txt
//   node scripts/check-api.mjs --update            rewrites api/*.txt from dist/ (commit it with the change)
//   node scripts/check-api.mjs --base <rev> [--title <pull request title>]
//                                                  a line of api/*.txt that <rev> had and HEAD lost is a
//                                                  breaking change: a commit in <rev>..HEAD (or the title) must
//                                                  be marked `!` / `BREAKING CHANGE:`, and CHANGELOG.md's
//                                                  [Unreleased] must gain a `- **BREAKING**` line
//
// A changed declaration shows as one line lost and one gained, so it is breaking too. Parameter names and
// macro values (the version) are left out: renaming a parameter or releasing is not a change of interface.
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { commitsIn, unreleased } from './check-commits.mjs'
import { isMain, parse } from './lib/conventional.mjs'
import { ROOT } from './release.mjs'

export const BASELINES = { 'api/c-abi.txt': cSurface, 'api/node.txt': nodeSurface }

const HEADER_NOTE = (what) => `# ${what}: compared with the build by scripts/check-api.mjs (docs/VERSIONING.md). Regenerate with
# \`node scripts/check-api.mjs --update\`; a line that goes away is a breaking change.
`

/** zeropty.h as lines: `macro NAME` per #define (not the include guard), `decl ...` per declaration, with
 *  comments, parameter names and whitespace differences left out. */
export function headerSurface(text) {
  const out = []
  const src = text.replace(/\/\*[\s\S]*?\*\//g, ' ').replace(/\/\/[^\n]*/g, ' ')
  const code = []
  for (const line of src.split('\n')) {
    const t = line.trim()
    if (t.startsWith('#')) {
      const m = /^#\s*define\s+([A-Za-z_]\w*)/.exec(t)
      if (m && !/_H$/.test(m[1])) out.push(`macro ${m[1]}`)
      continue
    }
    code.push(t)
  }
  const body = code.join(' ').replace(/extern\s+"C"\s*\{/g, ' ').replace(/(^|[;}\s])\}/g, '$1 ')
  for (const raw of body.split(';')) {
    let d = raw.replace(/\s+/g, ' ').trim()
    if (!d || d === '}') continue
    d = d.replace(/\s*\*\s*/g, ' *').replace(/\(\s*/g, '(').replace(/\s*\)/g, ')').replace(/\s*,\s*/g, ', ')
    const fn = /^(.*?)\((.*)\)$/.exec(d)
    if (fn) {
      const params = fn[2].split(', ').map((p) => {
        if (p === 'void' || p === '...') return p
        const m = /^(.*?[\s*])([A-Za-z_]\w*)$/.exec(p)
        return m && m[1].trim() ? m[1].trim() : p
      })
      d = `${fn[1]}(${params.join(', ')})`
    }
    out.push(`decl ${d}`)
  }
  return out
}

/** `symbol name` for each zeropty_* function the dylib exports. */
export function symbolSurface(nmOutput) {
  return nmOutput
    .split('\n')
    .map((l) => /\s[TDSB]\s+_?(zeropty_\w+)$/.exec(l.trim())?.[1])
    .filter(Boolean)
    .map((s) => `symbol ${s}`)
}

function cSurface(dist) {
  const nm = execFileSync('nm', ['-gU', path.join(dist, 'libzeropty_c.dylib')], { encoding: 'utf8' })
  const header = readFileSync(path.join(dist, 'zeropty.h'), 'utf8')
  return [...symbolSurface(nm), ...headerSurface(header).map((l) => `header ${l}`)]
}

/** What the Node modules export: `<module> <name> <kind>`, with a class's methods. Loaded in a child process,
 *  so this script does not load the native module itself. */
function nodeSurface(dist) {
  const script = `
    import { createRequire } from 'node:module'
    const require = createRequire(process.cwd() + '/')
    const lines = []
    const describe = (mod, name, v) => {
      if (typeof v !== 'function') return lines.push(mod + ' ' + name + ' ' + typeof v)
      const methods = v.prototype ? Object.getOwnPropertyNames(v.prototype).filter((m) => m !== 'constructor').sort() : []
      lines.push(mod + ' ' + name + (methods.length ? ' class ' + methods.join(',') : ' function'))
    }
    const native = require(process.env.ZEROPTY_NODE)
    for (const k of Object.keys(native)) describe('zeropty.node', k, native[k])
    for (const f of ['client/zeropty.mjs', 'client/embedded.mjs']) {
      const m = await import(process.cwd() + '/' + f)
      for (const k of Object.keys(m)) describe(f, k, m[k])
    }
    console.log(lines.join('\\n'))`
  const env = { ...process.env, ZEROPTY_NODE: path.join(dist, 'zeropty.node') }
  return execFileSync(process.execPath, ['--input-type=module', '-e', script], { cwd: ROOT, env, encoding: 'utf8' }).trim().split('\n')
}

const lines = (text) => (text ?? '').split('\n').filter((l) => l && !l.startsWith('#'))
const sorted = (xs) => [...new Set(xs)].sort()

/** { removed, added } between two surfaces. */
export function compare(before, after) {
  const a = new Set(before)
  const b = new Set(after)
  return { removed: [...a].filter((l) => !b.has(l)), added: [...b].filter((l) => !a.has(l)) }
}

function fileAt(rev, rel) {
  try {
    return execFileSync('git', ['-C', ROOT, 'show', `${rev}:${rel}`], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
  } catch {
    return null
  }
}

/** Problems with removals since `base`: none when nothing was removed, or when it is marked as breaking. */
export function breakingProblems({ removed, messages, changelogBefore, changelogAfter }) {
  if (!removed.length) return []
  const problems = []
  if (!messages.some((m) => parse(m).breaking)) {
    problems.push('no commit (nor the pull request title) is marked breaking: use `type(scope)!: ...` or a `BREAKING CHANGE:` footer')
  }
  const old = new Set(unreleased(changelogBefore).split('\n'))
  if (!unreleased(changelogAfter).split('\n').some((l) => !old.has(l) && /^\s*- \*\*BREAKING\*\*/.test(l))) {
    problems.push('CHANGELOG.md: add a "- **BREAKING** ..." line under ## [Unreleased] saying what users must change')
  }
  return problems
}

function main(argv) {
  const opt = { dist: path.join(ROOT, 'dist') }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === '--update') opt.update = true
    else if (a === '--base') opt.base = argv[++i]
    else if (a === '--title') opt.title = argv[++i]
    else if (a === '--dist') opt.dist = path.resolve(argv[++i])
    else {
      console.error(`check-api: unknown argument ${a}`)
      return 2
    }
  }

  if (opt.base) {
    let removed = []
    for (const rel of Object.keys(BASELINES)) {
      const before = fileAt(opt.base, rel)
      if (before === null) continue
      const d = compare(lines(before), lines(readFileSync(path.join(ROOT, rel), 'utf8')))
      for (const l of d.added) console.log(`+ ${rel}: ${l}`)
      for (const l of d.removed) console.log(`- ${rel}: ${l}`)
      removed = removed.concat(d.removed)
    }
    const messages = commitsIn(`${opt.base}..HEAD`).map((c) => c.message)
    if (opt.title) messages.push(opt.title)
    const problems = breakingProblems({
      removed,
      messages,
      changelogBefore: fileAt(execFileSync('git', ['-C', ROOT, 'merge-base', opt.base, 'HEAD'], { encoding: 'utf8' }).trim(), 'CHANGELOG.md'),
      changelogAfter: readFileSync(path.join(ROOT, 'CHANGELOG.md'), 'utf8'),
    })
    for (const p of problems) console.error(`✗ ${removed.length} line(s) of the public API removed or changed: ${p}`)
    if (!problems.length) console.log(removed.length ? 'api: breaking change, marked as such' : 'api: nothing removed since the base (no breaking change)')
    return problems.length ? 1 : 0
  }

  for (const f of ['libzeropty_c.dylib', 'zeropty.h', 'zeropty.node']) {
    if (!existsSync(path.join(opt.dist, f))) {
      console.error(`check-api: ${path.join(opt.dist, f)} is missing: run ./build.sh first`)
      return 2
    }
  }
  let failed = 0
  for (const [rel, surface] of Object.entries(BASELINES)) {
    const now = sorted(surface(opt.dist))
    const file = path.join(ROOT, rel)
    if (opt.update) {
      writeFileSync(file, HEADER_NOTE(rel === 'api/c-abi.txt' ? 'C ABI (libzeropty_c.dylib, zeropty.h)' : 'Node API') + now.join('\n') + '\n')
      console.log(`updated ${rel} (${now.length} lines)`)
      continue
    }
    const d = compare(existsSync(file) ? lines(readFileSync(file, 'utf8')) : [], now)
    if (!d.removed.length && !d.added.length) {
      console.log(`${rel}: the build matches (${now.length} lines)`)
      continue
    }
    failed++
    console.error(`✗ ${rel} does not match the build in dist/:`)
    for (const l of d.removed) console.error(`    - ${l}   (gone: a breaking change, docs/VERSIONING.md)`)
    for (const l of d.added) console.error(`    + ${l}`)
  }
  if (failed) console.error('  If the change is meant: node scripts/check-api.mjs --update, and commit api/ with it (a removal needs `!` and a **BREAKING** CHANGELOG line).')
  return failed ? 1 : 0
}

if (isMain(import.meta.url)) {
  try {
    process.exit(main(process.argv.slice(2)))
  } catch (e) {
    console.error(`check-api: ${e.message}`)
    process.exit(1)
  }
}
