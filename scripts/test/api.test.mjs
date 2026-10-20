// node --test scripts/test: scripts/check-api.mjs, the machine check of docs/VERSIONING.md's C ABI and Node API.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { describe, test } from 'node:test'
import { breakingProblems, compare, headerSurface, symbolSurface } from '../check-api.mjs'
import { ROOT } from '../release.mjs'

describe('C header surface', () => {
  const h = `/* a comment with zeropty_gone(void); in it */
#ifndef ZEROPTY_H
#define ZEROPTY_H
#ifdef __cplusplus
extern "C" {
#endif
#define ZEROPTY_VERSION_MAJOR 0
typedef struct zeropty zeropty;
// one more comment
char *zeropty_command(zeropty *a,
                      const char *request_json);
const char *zeropty_version(void);
#ifdef __cplusplus
}
#endif
#endif`

  test('declarations without names, comments or layout', () => {
    assert.deepEqual(headerSurface(h), [
      'macro ZEROPTY_VERSION_MAJOR',
      'decl typedef struct zeropty zeropty',
      'decl char *zeropty_command(zeropty *, const char *)',
      'decl const char *zeropty_version(void)',
    ])
  })

  test('a renamed parameter or another version is no change; another type is', () => {
    const renamed = h.replace('zeropty *a,', 'zeropty *engine,').replace('MAJOR 0', 'MAJOR 7')
    assert.deepEqual(compare(headerSurface(h), headerSurface(renamed)), { removed: [], added: [] })
    const retyped = h.replace('const char *request_json', 'const char *request_json, int flags')
    assert.deepEqual(compare(headerSurface(h), headerSurface(retyped)), {
      removed: ['decl char *zeropty_command(zeropty *, const char *)'],
      added: ['decl char *zeropty_command(zeropty *, const char *, int)'],
    })
  })

  test('the real header parses into every function the library exports', () => {
    const decls = headerSurface(readFileSync(path.join(ROOT, 'bindings/c/include/zeropty.h'), 'utf8'))
    const baseline = readFileSync(path.join(ROOT, 'api/c-abi.txt'), 'utf8')
    for (const sym of baseline.split('\n').filter((l) => l.startsWith('symbol '))) {
      const name = sym.slice('symbol '.length)
      assert.ok(decls.some((d) => d.includes(` ${name}(`) || d.includes(`*${name}(`)), `zeropty.h does not declare ${name}`)
    }
  })
})

test('nm output: exported zeropty_* functions only', () => {
  const nm = '0000000000008750 T _zeropty_close\n0000000000009318 T _zeropty_version\n0000000000001000 T _rust_helper\n                 U _malloc'
  assert.deepEqual(symbolSurface(nm), ['symbol zeropty_close', 'symbol zeropty_version'])
})

describe('removals need a breaking mark', () => {
  const before = '## [Unreleased]\n\n### Added\n\n- x\n\n## [0.1.0] - 2026-10-20\n'
  const marked = '## [Unreleased]\n\n### Changed\n\n- **BREAKING** zeropty_free is gone: use zeropty_release\n\n### Added\n\n- x\n\n## [0.1.0] - 2026-10-20\n'

  test('nothing removed: nothing asked', () => {
    assert.deepEqual(breakingProblems({ removed: [], messages: ['feat(c-abi): add zeropty_x'], changelogBefore: before, changelogAfter: before }), [])
  })
  test('removed without marks: both asked for', () => {
    assert.equal(breakingProblems({ removed: ['symbol zeropty_free'], messages: ['feat(c-abi): rename'], changelogBefore: before, changelogAfter: before }).length, 2)
  })
  test('removed, with ! or a footer, and a BREAKING line', () => {
    for (const m of ['feat(c-abi)!: rename zeropty_free', 'feat(c-abi): rename\n\nBREAKING CHANGE: zeropty_free is gone']) {
      assert.deepEqual(breakingProblems({ removed: ['symbol zeropty_free'], messages: [m], changelogBefore: before, changelogAfter: marked }), [])
    }
  })
  test('a BREAKING line that was there before does not count', () => {
    assert.equal(breakingProblems({ removed: ['symbol zeropty_free'], messages: ['fix!: x'], changelogBefore: marked, changelogAfter: marked }).length, 1)
  })
})

test('the build in dist/ matches api/*.txt', { skip: !existsSync(path.join(ROOT, 'dist/zeropty.node')) && 'no dist/ (run ./build.sh)' }, () => {
  execFileSync(process.execPath, [path.join(ROOT, 'scripts/check-api.mjs')], { stdio: ['ignore', 'pipe', 'pipe'] })
})
