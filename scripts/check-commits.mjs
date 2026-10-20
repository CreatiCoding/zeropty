#!/usr/bin/env node
// Checks commit messages against Conventional Commits (scripts/lib/conventional.mjs) and, for a pull
// request, that user-visible commits come with a CHANGELOG.md entry. CONTRIBUTING.md has the rules.
//
//   node scripts/check-commits.mjs --file .git/COMMIT_EDITMSG        # one message (the commit-msg hook)
//   node scripts/check-commits.mjs --message "feat(cli): add --version"   # e.g. a pull request title
//   node scripts/check-commits.mjs --range origin/main..HEAD [--changelog]
//
// --changelog: when the range has a feat/fix/perf/revert or breaking commit, CHANGELOG.md's [Unreleased]
// section must change in the range, and its new entries must sit where `release.mjs notes` (git-cliff)
// files those commits: feat under Added, fix under Fixed (fix(security) under Security), perf/revert under
// Changed, a breaking change as a "- **BREAKING** ..." line under Changed or Removed. The entries are
// rewritten in English for users (with a Korean copy in docs/ko/CHANGELOG.md), not copied from the draft,
// so only their sections are compared with the draft, not their words (docs/RELEASING.md). [Unreleased] may only use the Keep a Changelog headings, in their order.
// A commit with a "Changelog: skip" footer needs no entry (a fix to a feature still under [Unreleased]);
// ZEROPTY_SKIP_CHANGELOG=1 skips the entry checks for the whole range (the maintainer sets it for a pull request with nothing user-visible: `ZEROPTY_SKIP_CHANGELOG=1 scripts/check.sh`).
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { isMain, isMerge, parse, sectionsFor, SECTIONS, TYPES } from './lib/conventional.mjs'

const git = (...args) => execFileSync('git', args, { encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'pipe'] })

/** The [Unreleased] section of a CHANGELOG.md text, without its heading; '' when there is none. */
export function unreleased(text) {
  const m = /^## \[Unreleased\][^\n]*\n([\s\S]*?)(?=^## \[|^\[[^\]]+\]: |(?![\s\S]))/m.exec(text ?? '')
  return m ? m[1].trim() : ''
}

/**
 * An [Unreleased] body split into Keep a Changelog sections: { sections: Map(name -> entries), errors }.
 * An entry is a "- " line with its indented continuation lines, joined; errors lists headings that are
 * not Keep a Changelog's, out of order or repeated, and entries outside any heading.
 */
export function sections(body) {
  const out = new Map()
  const errors = []
  let current = null
  let last = -1
  let entry = null
  for (const line of (body ?? '').split('\n')) {
    const h = /^###\s+(.*?)\s*$/.exec(line)
    if (h) {
      entry = null
      const i = SECTIONS.indexOf(h[1])
      if (i < 0) errors.push(`"### ${h[1]}" is not one of ${SECTIONS.join(', ')}`)
      else if (out.has(h[1])) errors.push(`"### ${h[1]}" appears twice`)
      else if (i < last) errors.push(`"### ${h[1]}" comes after "### ${SECTIONS[last]}" (order: ${SECTIONS.join(', ')})`)
      current = i < 0 ? null : h[1]
      if (i >= 0) {
        last = Math.max(last, i)
        if (!out.has(current)) out.set(current, [])
      }
      continue
    }
    if (/^- /.test(line)) {
      if (!current) {
        errors.push(`entry outside a ### section: ${line.slice(0, 60)}`)
        entry = null
        continue
      }
      entry = { text: line.slice(2).trim() }
      out.get(current).push(entry)
    } else if (entry && /^\s+\S/.test(line)) entry.text += ` ${line.trim()}`
    else if (line.trim()) entry = null
  }
  for (const [name, list] of out) out.set(name, list.map((e) => e.text))
  return { sections: out, errors }
}

/**
 * What is wrong with the [Unreleased] change from `before` to `after` (two CHANGELOG.md texts) for the
 * parsed `commits`: each user-visible commit's section (sectionsFor, the git-cliff draft's grouping) must
 * gain an entry, and a breaking one a **BREAKING** entry under Changed or Removed. [] when it matches.
 * The headings themselves are checked by sections().
 */
export function changelogProblems(before, after, commits) {
  const old = sections(unreleased(before)).sections
  const now = sections(unreleased(after)).sections
  const problems = []
  const added = new Map()
  for (const [name, list] of now) {
    const seen = new Set(old.get(name) ?? [])
    added.set(name, list.filter((e) => !seen.has(e)))
  }
  const visible = commits.filter((c) => sectionsFor(c))
  if (!visible.length) return problems
  if (unreleased(before) === unreleased(after)) {
    problems.push(`${visible.length} user-visible commit(s) but CHANGELOG.md's [Unreleased] did not change`)
    return problems
  }
  const label = (c) => `${c.type}${c.scope ? `(${c.scope})` : ''}${c.breaking ? '!' : ''}: ${c.subject}`
  const missing = new Map()
  for (const c of visible) {
    const want = sectionsFor(c)
    const ok = c.breaking
      ? want.some((s) => (added.get(s) ?? []).some((e) => e.startsWith('**BREAKING**')))
      : want.some((s) => (added.get(s) ?? []).length)
    if (!ok) {
      const key = c.breaking ? '"- **BREAKING** ..." under ### Changed or ### Removed' : `an entry under ### ${want[0]}`
      if (!missing.has(key)) missing.set(key, [])
      missing.get(key).push(label(c))
    }
  }
  for (const [key, list] of missing) problems.push(`needs ${key} for: ${list.join('; ')}`)
  return problems
}

/** CHANGELOG.md at a revision; null when it does not exist there. */
function changelogAt(rev) {
  try {
    return git('show', `${rev}:CHANGELOG.md`)
  } catch {
    return null
  }
}

/** [{ sha, message }] for a range, oldest first, merges left out. */
export function commitsIn(range) {
  const out = git('log', '--reverse', '--no-merges', '--format=%H%x00%B%x01', range)
  return out
    .split('\x01')
    .map((s) => s.replace(/^\n/, ''))
    .filter((s) => s.includes('\x00'))
    .map((s) => {
      const [sha, message] = s.split('\x00')
      return { sha, message: message.trim() }
    })
    .filter((c) => !isMerge(c.message))
}

function report(label, c, problems) {
  console.error(`✗ ${label}: ${c.message.split('\n')[0]}`)
  for (const p of problems) console.error(`    ${p}`)
}

function main(argv) {
  const opt = { changelog: false }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === '--file') opt.file = argv[++i]
    else if (a === '--message') opt.message = argv[++i]
    else if (a === '--range') opt.range = argv[++i]
    else if (a === '--changelog') opt.changelog = true
    else {
      console.error(`check-commits: unknown argument ${a}`)
      return 2
    }
  }
  const help = `  types: ${Object.keys(TYPES).join(', ')}; e.g. "feat(cli): add --version", "fix(term)!: ..." (CONTRIBUTING.md)`

  if (opt.file || opt.message !== undefined) {
    const message = opt.file ? readFileSync(opt.file, 'utf8') : opt.message
    // git writes these itself (merge, fixup/squash for a later rebase --autosquash); they are checked when squashed.
    if (isMerge(message) || /^(fixup|squash|amend)! /.test(message)) return 0
    const c = parse(message)
    if (c.ok) return 0
    report(opt.file ? 'commit message' : 'message', { message }, c.errors)
    console.error(help)
    return 1
  }

  if (!opt.range) {
    console.error('usage: check-commits.mjs --file <path> | --message <text> | --range <base>..<head> [--changelog]')
    return 2
  }
  const commits = commitsIn(opt.range)
  let failed = 0
  const parsed = []
  for (const c of commits) {
    const short = c.sha.slice(0, 7)
    if (/^(fixup|squash|amend)! /.test(c.message)) {
      report(short, c, ['fixup/squash commit: squash it before merging (git rebase -i --autosquash)'])
      failed++
      continue
    }
    const p = parse(c.message)
    parsed.push(p)
    if (!p.ok) {
      report(short, c, p.errors)
      failed++
    }
  }
  if (failed) console.error(help)
  console.log(`${commits.length - failed}/${commits.length} commits in ${opt.range} follow Conventional Commits`)

  if (opt.changelog) {
    const head = opt.range.split(/\.\.\.?/)[1] || 'HEAD'
    const shape = sections(unreleased(changelogAt(head))).errors
    for (const e of shape) console.error(`✗ changelog: [Unreleased]: ${e}`)
    failed += shape.length
    const visible = parsed.filter((c) => sectionsFor(c))
    if (!visible.length) {
      console.log('changelog: no user-visible commits (feat, fix, perf, revert, breaking; no "Changelog: skip"); no entry needed')
    } else if (process.env.ZEROPTY_SKIP_CHANGELOG === '1') {
      console.log(`changelog: skipped (ZEROPTY_SKIP_CHANGELOG=1) for ${visible.length} user-visible commit(s)`)
    } else {
      const [base, head] = opt.range.split(/\.\.\.?/)
      const from = git('merge-base', base, head || 'HEAD').trim()
      const problems = changelogProblems(changelogAt(from), changelogAt(head || 'HEAD'), parsed)
      if (problems.length) {
        console.error('✗ changelog: [Unreleased] does not match the commits (docs/RELEASING.md):')
        for (const p of problems) console.error(`    ${p}`)
        console.error('  `node scripts/release.mjs notes` shows where git-cliff files these commits; or, if nothing is user-visible, run with ZEROPTY_SKIP_CHANGELOG=1.')
        failed++
      } else {
        const want = [...new Set(visible.map((c) => (c.breaking ? 'BREAKING' : sectionsFor(c)[0])))]
        console.log(`changelog: [Unreleased] has new entries for ${want.join(', ')}`)
      }
    }
  }
  return failed ? 1 : 0
}

if (isMain(import.meta.url)) process.exit(main(process.argv.slice(2)))
