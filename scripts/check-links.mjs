// Checks the links between the repository's Markdown files: every relative link (Markdown `[text](path)`,
// `<a href>`, `<img src>`, `<source srcset>`) must name a file or directory that exists, and every `#anchor`
// must name a heading (GitHub's slug) or an explicit `<a id>`/`<a name>` in the file it points to.
// External links (http, https, mailto) are not fetched. Code blocks and inline code are skipped.
//
//   node scripts/check-links.mjs            every tracked *.md file
//   node scripts/check-links.mjs a.md b.md  only these
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..')
process.chdir(root)

const files = process.argv.slice(2).length
  ? process.argv.slice(2)
  : execFileSync('git', ['ls-files', '-co', '--exclude-standard', '*.md'], { encoding: 'utf8' }).split('\n').filter(Boolean).filter((f) => fs.existsSync(f))

/** The text with fenced code blocks and inline code blanked out (line numbers kept). */
function prose(text) {
  let fence = null
  return text
    .split('\n')
    .map((line) => {
      const m = line.match(/^\s*(`{3,}|~{3,})/)
      if (m) {
        if (!fence) fence = m[1][0]
        else if (m[1][0] === fence) fence = null
        return ''
      }
      if (fence) return ''
      return line.replace(/`+[^`]*`+/g, '``')
    })
    .join('\n')
}

/** GitHub's heading anchor: lowercased, punctuation dropped, spaces to hyphens; repeats get -1, -2, ... */
function slugs(text) {
  const seen = new Map()
  const out = new Set()
  let fence = null
  for (const line of text.split('\n')) {
    const f = line.match(/^\s*(`{3,}|~{3,})/)
    if (f) {
      if (!fence) fence = f[1][0]
      else if (f[1][0] === fence) fence = null
      continue
    }
    if (fence) continue
    for (const m of line.matchAll(/<a\s+[^>]*(?:id|name)="([^"]+)"/g)) out.add(m[1])
    const h = line.match(/^#{1,6}\s+(.*?)\s*#*\s*$/)
    if (!h) continue
    const plain = h[1]
      .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
      .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
      .replace(/<[^>]+>/g, '')
      .replace(/`/g, '')
      .replace(/\*\*|__|\*/g, '')
    let slug = plain
      .toLowerCase()
      .replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, '')
      .replace(/ /g, '-')
    const n = seen.get(slug) ?? 0
    seen.set(slug, n + 1)
    if (n) slug = `${slug}-${n}`
    out.add(slug)
  }
  return out
}

const anchorCache = new Map()
const anchorsOf = (file) => {
  if (!anchorCache.has(file)) anchorCache.set(file, slugs(fs.readFileSync(file, 'utf8')))
  return anchorCache.get(file)
}

const problems = []
let checked = 0
for (const file of files) {
  const text = prose(fs.readFileSync(file, 'utf8'))
  const lines = text.split('\n')
  lines.forEach((line, i) => {
    const targets = []
    for (const m of line.matchAll(/\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)) targets.push(m[1])
    for (const m of line.matchAll(/\b(?:href|src)="([^"]+)"/g)) targets.push(m[1])
    for (const m of line.matchAll(/\bsrcset="([^"]+)"/g)) targets.push(...m[1].split(',').map((s) => s.trim().split(/\s+/)[0]))
    for (const t of targets) {
      if (/^[a-z][a-z0-9+.-]*:/i.test(t)) continue // http:, https:, mailto:, ...
      checked++
      const [p, anchor] = t.split('#')
      const target = p ? path.normalize(path.join(path.dirname(file), decodeURIComponent(p))) : file
      const where = `${file}:${i + 1}`
      if (p && !fs.existsSync(target)) {
        problems.push(`${where}: ${t}: no such file`)
        continue
      }
      if (anchor !== undefined && anchor !== '' && target.endsWith('.md')) {
        if (!anchorsOf(target).has(decodeURIComponent(anchor))) problems.push(`${where}: ${t}: no heading #${anchor} in ${target}`)
      }
    }
  })
}

for (const p of problems) console.error(p)
if (problems.length) {
  console.error(`check-links: ${problems.length} broken link(s) in ${files.length} file(s)`)
  process.exit(1)
}
console.log(`check-links: ${checked} relative link(s) in ${files.length} file(s), all found`)
