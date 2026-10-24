// Writes llms.txt (every page with its one-line description) and llms-full.txt (every page's Markdown)
// into the built site, for tools that read documentation as text. Usage: node scripts/llms.mjs SITE_URL
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const src = join(site, 'src')
const dist = join(src, '.vitepress/dist')
const origin = process.argv[2].replace(/\/$/, '')

const files = []
;(function walk(dir) {
  for (const name of readdirSync(dir).sort()) {
    if (name.startsWith('.') || name === 'public') continue
    const file = join(dir, name)
    if (statSync(file).isDirectory()) walk(file)
    else if (name.endsWith('.md')) files.push(file)
  }
})(src)

const pages = []
for (const file of files) {
  const text = readFileSync(file, 'utf8')
  const front = text.match(/^---\n([\s\S]*?)\n---\n/)
  if (front && /^layout: page$/m.test(front[1])) continue // the home page is a component, not prose
  const body = front ? text.slice(front[0].length) : text
  const title = body.match(/^# (.+)$/m)?.[1].replace(/<[^>]*>/g, '').trim()
  if (!title) continue
  const description = front?.[1].match(/^description: (.+)$/m)?.[1].trim() ?? ''
  const path = '/' + relative(src, file).replace(/(^|\/)index\.md$/, '$1').replace(/\.md$/, '')
  pages.push({ title, description, url: origin + path, body: body.trim() })
}

// Reading order: the sidebar's, roughly. Pages not listed keep their place after the listed ones.
const order = ['introduction', 'getting-started', 'requirements', 'install', '/concepts/', '/guide/', '/use/', '/operate/', '/reference/', 'changelog']
const rank = (url) => {
  const i = order.findIndex((key) => url.includes(key))
  return i < 0 ? order.length : i
}
pages.sort((a, b) => rank(a.url) - rank(b.url))

const name = readFileSync(join(src, 'index.md'), 'utf8').match(/^title: (.+)$/m)?.[1] ?? 'Docs'
writeFileSync(
  join(dist, 'llms.txt'),
  [`# ${name}`, '', ...pages.map((p) => `- [${p.title}](${p.url})${p.description ? `: ${p.description}` : ''}`), ''].join('\n'),
)
writeFileSync(join(dist, 'llms-full.txt'), pages.map((p) => `<!-- ${p.url} -->\n\n${p.body}\n`).join('\n---\n\n'))
console.log(`llms.txt: ${pages.length} pages`)
