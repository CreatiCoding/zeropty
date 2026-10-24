// Fails the build when a link inside the site points at a heading that does not exist. VitePress checks
// that linked pages exist, not the #anchor part, and the docs' anchors were written against GitHub.
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const dist = resolve(dirname(fileURLToPath(import.meta.url)), '../src/.vitepress/dist')
const pages = new Map() // site path ("/guide/usage") -> set of ids
function walk(dir) {
  for (const name of readdirSync(dir)) {
    const file = join(dir, name)
    if (statSync(file).isDirectory()) walk(file)
    else if (name.endsWith('.html')) {
      const path = '/' + file.slice(dist.length + 1).replace(/(index)?\.html$/, '')
      const html = readFileSync(file, 'utf8')
      pages.set(path, { html, ids: new Set([...html.matchAll(/\bid="([^"]+)"/g)].map((m) => decodeURIComponent(m[1]))) })
    }
  }
}
walk(dist)

const broken = []
for (const [path, { html }] of pages) {
  for (const [, href] of html.matchAll(/href="([^"]*#[^"]+)"/g)) {
    const [target, anchor] = href.split('#')
    if (/^[a-z]+:/i.test(target)) continue
    const page = pages.get(target === '' ? path : target)
    if (!page?.ids.has(decodeURIComponent(anchor))) broken.push(`${path} -> ${href}`)
  }
}
if (broken.length) {
  console.error(`broken anchors (${broken.length}):\n  ${broken.join('\n  ')}`)
  process.exit(1)
}
console.log(`anchors ok across ${pages.size} pages`)
