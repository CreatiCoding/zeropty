// Conventional Commits (https://www.conventionalcommits.org/en/v1.0.0/) as zeropty uses them: the types,
// how a message is parsed, and which commits a release and the changelog care about. Shared by
// scripts/check-commits.mjs, scripts/release.mjs and their tests; no dependencies.
import { realpathSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

/** Whether the module at `url` is the script node was started with (not imported by a test). */
export function isMain(url) {
  try {
    return pathToFileURL(realpathSync(process.argv[1])).href === url
  } catch {
    return false
  }
}

/** Allowed types, and what each means for users (CONTRIBUTING.md). */
export const TYPES = {
  feat: 'a new capability (command, flag, API, env var)',
  fix: 'a bug fix',
  perf: 'faster or smaller, same behaviour',
  refactor: 'code change with no behaviour change',
  docs: 'documentation only',
  test: 'tests only',
  build: 'build scripts, Cargo manifests, dependencies',
  ci: 'check script and git hooks (scripts/check.sh, .githooks/)',
  chore: 'anything else that users never see (release commits too)',
  style: 'formatting only',
  revert: 'reverts an earlier commit',
}

/** Types that users notice: a pull request with one of them adds a line to CHANGELOG.md's [Unreleased]. */
export const USER_VISIBLE = new Set(['feat', 'fix', 'perf', 'revert'])

export const MAX_HEADER = 100

const HEADER = /^(?<type>[a-z]+)(?:\((?<scope>[a-z0-9][a-z0-9._/-]*)\))?(?<bang>!)?: (?<subject>\S.*)$/
const BREAKING_FOOTER = /^BREAKING[ -]CHANGE: \S/m
// A footer for a commit whose change users never see in a release, e.g. a fix to a feature that is still
// under [Unreleased]: no CHANGELOG.md entry is asked for and git-cliff leaves it out (cliff.toml). Not for
// breaking changes, which always need a **BREAKING** entry.
const SKIP_FOOTER = /^Changelog: skip$/m

/** Merge commits (git's and GitHub's default subjects): not checked, their parents are. */
export function isMerge(message) {
  return /^Merge (branch|pull request|remote-tracking branch|tag) /.test(message)
}

/**
 * Parses a commit message. Returns { ok, type, scope, breaking, subject, errors }: errors lists every rule
 * the message breaks (empty when ok).
 */
export function parse(message) {
  const text = message.replace(/\r\n/g, '\n').replace(/^#.*\n?/gm, '').replace(/\s+$/, '')
  const [header = '', ...rest] = text.split('\n')
  const errors = []
  const m = HEADER.exec(header)
  if (!m) {
    errors.push(`header "${header}" is not "<type>(<scope>)!: <subject>" (scope and ! optional)`)
    return { ok: false, type: null, scope: null, breaking: false, subject: header, skipChangelog: false, errors }
  }
  const { type, scope = null, bang, subject } = m.groups
  if (!(type in TYPES)) errors.push(`type "${type}" is not one of ${Object.keys(TYPES).join(', ')}`)
  if (header.length > MAX_HEADER) errors.push(`header is ${header.length} characters (at most ${MAX_HEADER})`)
  if (/\.$/.test(subject)) errors.push('subject ends with a period')
  if (rest.length && rest[0] !== '') errors.push('no blank line between the header and the body')
  const body = rest.join('\n')
  const breaking = Boolean(bang) || BREAKING_FOOTER.test(body)
  const skipChangelog = SKIP_FOOTER.test(body)
  if (skipChangelog && breaking) errors.push('a breaking change cannot have "Changelog: skip": it needs a **BREAKING** entry')
  return { ok: errors.length === 0, type, scope, breaking, subject, skipChangelog, errors }
}

/** Whether a parsed commit needs a CHANGELOG.md entry. */
export function userVisible(c) {
  return c.ok && (c.breaking || USER_VISIBLE.has(c.type))
}

/** Keep a Changelog sections, in the order CHANGELOG.md lists them (cliff.toml orders its groups the same). */
export const SECTIONS = ['Added', 'Changed', 'Deprecated', 'Removed', 'Fixed', 'Security']

/**
 * The CHANGELOG.md section a user-visible commit belongs under: the same mapping as cliff.toml's
 * commit_parsers (scripts/test/cliff.test.mjs renders both and compares them). A breaking change goes
 * under Changed, or under Removed when it removes something, so both are returned for it; null for a
 * commit users never see or one with a "Changelog: skip" footer.
 */
export function sectionsFor(c) {
  if (!userVisible(c) || c.skipChangelog) return null
  if (c.breaking) return ['Changed', 'Removed']
  if (c.type === 'feat') return ['Added']
  if (c.type === 'fix') return [c.scope === 'security' ? 'Security' : 'Fixed']
  return ['Changed'] // perf, revert
}

/**
 * The bump a set of parsed commits calls for, under docs/VERSIONING.md: before 1.0 a breaking change bumps
 * minor and everything else patch; from 1.0 breaking -> major, feat -> minor, fix/perf/revert -> patch.
 * null when nothing user-visible happened.
 */
export function bumpFor(commits, major) {
  const seen = commits.filter(userVisible)
  if (!seen.length) return null
  const breaking = seen.some((c) => c.breaking)
  if (major === 0) return breaking ? 'minor' : 'patch'
  if (breaking) return 'major'
  return seen.some((c) => c.type === 'feat') ? 'minor' : 'patch'
}
