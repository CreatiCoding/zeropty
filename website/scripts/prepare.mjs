// Builds the generated half of the site from the repository's own docs, so it always matches main:
// the reference (sections of docs/USAGE.md and docs/PLUGINS.md, and api/*.txt) and the project pages
// (benchmarks, design notes, FAQ, ...). The guide and concept pages are written for the site, in src/.
// Links are pointed at the page that now holds each heading, or at GitHub for anything else.
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, normalize, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repo = resolve(site, '..')
const blob = 'https://github.com/CreatiCoding/zeropty/blob/main/'
const read = (path) => readFileSync(join(repo, path), 'utf8')
const docs = (lang, name) => (lang === 'ko' ? `docs/ko/${name}` : `docs/${name}`)

// The same GitHub slugs as .vitepress/config.mts, so anchors written for GitHub keep working.
const slug = (text) =>
  text.trim().toLowerCase().replace(/<[^>]*>/g, '').replace(/[^\p{L}\p{N}\p{M} _-]/gu, '').replace(/ /g, '-')

// Lines of a Markdown file split into its preamble and its H2 sections, code fences respected.
function sections(text) {
  const out = [[]]
  let fence = null
  for (const line of text.split('\n')) {
    const f = line.match(/^\s*(`{3,}|~{3,})/)
    if (f) fence = fence ? (f[1][0] === fence ? null : fence) : f[1][0]
    if (!fence && line.startsWith('## ')) out.push([])
    out.at(-1).push(line)
  }
  return out.map((lines) => lines.join('\n'))
}
const pick = (path, indexes) => {
  const parts = sections(read(path)).slice(1)
  return indexes.map((i) => parts[i]).join('\n\n')
}
// A section without one of its H3 subsections (up to the next H3 or H2).
function dropH3(text, heading) {
  const lines = text.split('\n')
  const start = lines.findIndex((l) => heading.test(l))
  if (start < 0) return text
  let end = start + 1
  while (end < lines.length && !/^#{2,3} /.test(lines[end])) end++
  return [...lines.slice(0, start), ...lines.slice(end)].join('\n')
}
const ownAnchors = (text, file) => text.replace(/\]\(#/g, `](${file}#`)
const code = (path, lang) => `\`\`\`${lang}\n${read(path).trimEnd()}\n\`\`\``

// [route, title, intro, body, source] per language. USAGE and PLUGINS have the same H2 order in both languages.
const t = (en, ko) => ({ en, ko })
const pages = [
  ['/reference/commands', t('Commands', '명령'),
    t('Every command and option of the `zeropty` CLI. The same commands work over the socket and through the embedding APIs.', '`zeropty` CLI 의 모든 명령과 옵션이에요. 같은 명령이 소켓과 임베딩 API 에서도 그대로 돼요.'),
    (l) => pick(docs(l, 'USAGE.md'), [1, 2]), (l) => docs(l, 'USAGE.md')],
  ['/reference/keys-and-formats', t('Keys, formats and exit codes', '키, 포맷, 종료 코드'),
    t('Key names for `send-keys`, format variables for `-F`, and the exit codes of every command.', '`send-keys` 의 키 이름, `-F` 의 포맷 변수, 모든 명령의 종료 코드예요.'),
    (l) => pick(docs(l, 'USAGE.md'), [3, 4, 5]), (l) => docs(l, 'USAGE.md')],
  ['/reference/environment', t('Environment variables', '환경변수'),
    t('Every environment variable zeropty reads.', 'zeropty 가 읽는 모든 환경변수예요.'),
    (l) => pick(docs(l, 'USAGE.md'), [12]), (l) => docs(l, 'USAGE.md')],
  ['/reference/socket-protocol', t('Socket protocol', '소켓 프로토콜'),
    t('The JSON protocol on the server socket, including moving sessions between servers.', '서버 소켓의 JSON 프로토콜이에요. 서버 사이에 세션을 옮기는 부분도 있어요.'),
    (l) => dropH3(pick(docs(l, 'USAGE.md'), [9, 11]), /^### Node (client|클라이언트)$/), (l) => docs(l, 'USAGE.md')],
  ['/reference/rust-api', t('Rust API', 'Rust API'),
    t('The Rust crate: the engine inside your process, the server-plugin variant, and plugins written in Rust. For the generated rustdoc, run cargo doc as below.', 'Rust 크레이트예요. 내 프로세스 안의 엔진, 서버 플러그인으로 쓰는 법, Rust 로 쓴 플러그인이 있어요. 생성된 rustdoc 은 아래처럼 cargo doc 으로 봐요.'),
    // Parts of three files in docs/ (or docs/ko/): a link to a heading of its own file names that file,
    // since the page's links are read as from USAGE.md, which sits in the same directory.
    (l) => [
      ownAnchors(pick(docs(l, 'README.md'), [3]), 'README.md'),
      pick(docs(l, 'USAGE.md'), [10]),
      ownAnchors(pick(docs(l, 'PLUGINS.md'), [7]), 'PLUGINS.md').replace(/^## 4\. (Inside your process: Rust|내 프로세스 안에서: Rust)$/m, l === 'ko' ? '## Rust 로 쓴 플러그인' : '## Plugins in Rust'),
    ].join('\n\n'), (l) => docs(l, 'USAGE.md')],
  ['/reference/plugin-api', t('Plugin API', '플러그인 API'),
    t('Access rules, the socket protocol for plugins, events, commands and the Rust API of `zeropty::plugin`.', '권한, 플러그인용 소켓 프로토콜, 이벤트, 명령, `zeropty::plugin` 의 Rust API 예요.'),
    (l) => pick(docs(l, 'PLUGINS.md'), [3, 4, 9, 10, 11, 12, 15]), (l) => docs(l, 'PLUGINS.md')],
  ['/reference/tmux-compatibility', t('tmux compatibility', 'tmux 호환'),
    t('What zeropty does and does not do compared with tmux.', 'tmux 와 비교해 zeropty 가 하는 것과 하지 않는 것이에요.'),
    (l) => pick(docs(l, 'USAGE.md'), [13]), (l) => docs(l, 'USAGE.md')],
  // Project pages: whole files.
  ['/guide/benchmarks', null, null, (l) => read(docs(l, 'BENCHMARKS.md')), (l) => docs(l, 'BENCHMARKS.md')],
  ['/guide/design', null, null, (l) => read(docs(l, 'DESIGN.md')), (l) => docs(l, 'DESIGN.md')],
  ['/guide/faq', null, null, (l) => read(docs(l, 'FAQ.md')), (l) => docs(l, 'FAQ.md')],
  ['/guide/plugins-internals', null, null, (l) => read(docs(l, 'PLUGINS-INTERNALS.md')), (l) => docs(l, 'PLUGINS-INTERNALS.md')],
  ['/guide/versioning', null, null, (l) => read(docs(l, 'VERSIONING.md')), (l) => docs(l, 'VERSIONING.md')],
  ['/guide/releasing', null, null, (l) => read(docs(l, 'RELEASING.md')), (l) => docs(l, 'RELEASING.md')],
  ['/guide/changelog', null, null, (l) => read(l === 'ko' ? 'docs/ko/CHANGELOG.md' : 'CHANGELOG.md'), (l) => (l === 'ko' ? 'docs/ko/CHANGELOG.md' : 'CHANGELOG.md')],
  ['/guide/contributing', null, null, (l) => read(l === 'ko' ? 'docs/ko/CONTRIBUTING.md' : 'CONTRIBUTING.md'), (l) => (l === 'ko' ? 'docs/ko/CONTRIBUTING.md' : 'CONTRIBUTING.md')],
]

// Whole source files that map to a hand-written page, and README anchors that moved to one.
const fileRoutes = {
  'README.md': '/guide/introduction', 'README.ko.md': '/ko/guide/introduction',
  'docs/README.md': '/guide/introduction', 'docs/ko/README.md': '/ko/guide/introduction',
  'docs/USAGE.md': '/reference/commands', 'docs/ko/USAGE.md': '/ko/reference/commands',
  'docs/PLUGINS.md': '/reference/plugin-api', 'docs/ko/PLUGINS.md': '/ko/reference/plugin-api',
}
const readmeAnchors = { 'quick-start': '/guide/getting-started', '1분-시작': '/guide/getting-started', install: '/guide/install', '설치': '/guide/install' }

const prefix = (lang) => (lang === 'ko' ? '/ko' : '')
const built = []
// Text written for the repository that reads oddly on the site: the changelog's notes for contributors,
// DESIGN's list of its own diagrams, a pointer to a section the reference page does not carry.
function siteOnly(text, from) {
  if (/CHANGELOG\.md$/.test(from)) {
    const start = text.indexOf('\n## ')
    const ko = from.includes('/ko/')
    const intro = ko
      ? '사용자에게 보이는 변경을 판마다 적어요. 형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/), 판 번호는 [유의적 버전](https://semver.org/lang/ko/)을 따르고, 무엇이 깨는 변경인지는 [버전 정책](/ko/guide/versioning)에 있어요. 릴리스 파일은 [GitHub Releases](https://github.com/CreatiCoding/zeropty/releases)에 있어요.'
      : 'Every user-visible change, release by release. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow [Semantic Versioning](https://semver.org/); what counts as a breaking change is in [Versioning](/guide/versioning). Release files are on [GitHub Releases](https://github.com/CreatiCoding/zeropty/releases).'
    text = `${text.slice(0, text.indexOf('\n') + 1)}\n${intro}\n${text.slice(start)}`
  }
  if (/CHANGELOG\.md$/.test(from)) text = text.replace(/^(## \[\d[^\]]*\][^\n]*)$/m, `$1 <Badge type="tip" text="${from.includes('/ko/') ? '최신' : 'Latest'}" />`)
  text = text.replace(/^(Diagrams|그림): \[.*\n/m, '')
  text = text.replace(': section 2 above.', ': see [Write a plugin](/guide/plugins).')
  return text
}

for (const [route, title, intro, body, source] of pages) {
  for (const lang of ['en', 'ko']) {
    let text = siteOnly(body(lang), source(lang))
    if (title) text = `# ${title[lang]}\n\n${intro[lang]}\n\n${text}`
    built.push({ route: prefix(lang) + route, lang, from: source(lang), text })
  }
}

// Which page holds each heading of each source file.
const anchorPage = new Map() // `${source}#${slug}` -> route
for (const page of built) {
  let fence = null
  for (const line of page.text.split('\n')) {
    const f = line.match(/^\s*(`{3,}|~{3,})/)
    if (f) fence = fence ? (f[1][0] === fence ? null : fence) : f[1][0]
    const h = !fence && line.match(/^#{2,4} (.+)/)
    if (h) anchorPage.set(`${page.from}#${slug(h[1])}`, page.route)
  }
}
const titles = new Map(built.map((p) => [p.route, p.text.match(/^# (.+)$/m)?.[1]]))
;(function walk(dir) {
  for (const name of readdirSync(dir)) {
    const file = join(dir, name)
    if (name.startsWith('.') || name === 'public') continue
    if (statSync(file).isDirectory()) walk(file)
    else if (name.endsWith('.md') && !/^---\nsource: /.test(readFileSync(file, 'utf8'))) {
      const h1 = readFileSync(file, 'utf8').match(/^# (.+)$/m)?.[1]
      if (h1) titles.set('/' + relative(join(site, 'src'), file).replace(/\.md$/, ''), h1)
    }
  }
})(join(site, 'src'))
const wholeFile = new Map(built.filter((p) => !p.route.includes('/reference/')).map((p) => [p.from, p.route]))

function rewrite(target, from, lang) {
  if (/^([a-z]+:|\/)/i.test(target)) return target // external, or already a site path
  const [path, anchor] = target.split('#')
  const rel = path === '' ? from : relative(repo, normalize(join(dirname(join(repo, from)), decodeURI(path))))
  if (rel.startsWith('..')) return target
  const hash = anchor === undefined ? '' : `#${anchor}`
  if (anchor !== undefined && anchorPage.has(`${rel}#${decodeURIComponent(anchor)}`)) return anchorPage.get(`${rel}#${decodeURIComponent(anchor)}`) + hash
  if (/^README(\.ko)?\.md$/.test(rel) && anchor && readmeAnchors[decodeURIComponent(anchor)]) return prefix(lang) + readmeAnchors[decodeURIComponent(anchor)]
  if (wholeFile.has(rel)) return wholeFile.get(rel) + hash
  if (anchor === undefined && fileRoutes[rel]) return fileRoutes[rel]
  // Anything else, including a heading of this file that the site does not carry, is read on GitHub.
  return blob + encodeURI(rel) + hash
}

function adapt(text, from, lang) {
  const out = []
  const lines = text.split('\n')
  let fence = null
  for (let i = 0; i < lines.length; i++) {
    let line = lines[i]
    const f = line.match(/^\s*(`{3,}|~{3,})/)
    if (f) {
      fence = fence ? (f[1][0] === fence ? null : fence) : f[1][0]
      out.push(line)
      continue
    }
    if (fence) { out.push(line); continue }
    // Language switch and the nav line under each title: the site has its own.
    if (/^(\*\*English\*\* · \[한국어\]|\[English\]\(.*\) · \*\*한국어\*\*)/.test(line)) continue
    if (/^\[README\]\(.*\) · /.test(line)) continue
    // A table of contents written by hand: the site shows one beside the page.
    if (/^- \[[^\]]*\]\(#/.test(line) && (out.at(-1) ?? '') === '') {
      let j = i
      while (j < lines.length && /^\s*- \[[^\]]*\]\(#[^)]*\)( · \[[^\]]*\]\(#[^)]*\))*\s*$/.test(lines[j])) j++
      if (j - i >= 3 && (lines[j] ?? '') === '') { i = j; continue }
    }
    line = line.replace(/\[([^\]]*)\]\(([^)\s]+)\)/g, (_, label, target) => {
      const to = rewrite(target, from, lang)
      // [docs/VERSIONING.md](...) reads as a file on GitHub; on the site, name the page.
      const page = to.split('#')[0]
      if (/^`?[\w./-]+\.md(#[^`]*)?`?$/.test(label) && titles.has(page)) label = titles.get(page)
      return `[${label}](${to})`
    })
    line = line.replace(/\b(href|src)="([^"]+)"/g, (_, a, target) => `${a}="${rewrite(target, from, lang)}"`)
    out.push(line)
  }
  return out.join('\n')
}

// The first paragraph after the title, as plain text: the page's meta description.
function describe(text) {
  const para = text.split(/\n\s*\n/).find((p) => p.trim() && !/^\s*(#|<|\||-|\*\*English|\[English|\`\`\`|>)/.test(p))
  if (!para) return ''
  const plain = para.replace(/\[([^\]]*)\]\([^)]*\)/g, '$1').replace(/[\`*]/g, '').replace(/\\~/g, '~').replace(/\s+/g, ' ').trim()
  const cut = plain.length > 200 ? plain.slice(0, plain.lastIndexOf(' ', 197)) + '…' : plain
  return JSON.stringify(cut)
}

for (const dir of ['src/reference', 'src/ko/reference']) {
  if (!existsSync(join(site, dir))) continue
  for (const name of readdirSync(join(site, dir))) {
    const file = join(site, dir, name)
    if (/^---\nsource: /.test(readFileSync(file, 'utf8'))) rmSync(file)
  }
}
for (const page of built) {
  const to = join(site, 'src', `${page.route}.md`)
  mkdirSync(dirname(to), { recursive: true })
  const body = adapt(page.text, page.from, page.lang)
  writeFileSync(to, `---\nsource: ${page.from}\ndescription: ${describe(body)}\n---\n\n${body}\n`)
}

// The hand-written API pages must name every symbol the build checks (api/*.txt).
const missing = []
for (const [list, page] of [['api/c-abi.txt', 'reference/c-abi.md'], ['api/node.txt', 'reference/node-api.md']]) {
  const names = new Set()
  for (const line of read(list).split('\n')) {
    if (line.startsWith('#') || !line.trim()) continue
    const m = line.match(/^symbol (\w+)|^header macro (\w+)|typedef struct (\w+)|^\S+ (\w+) (?:function|string)|^\S+ (\w+) class (\S+)/)
    if (!m) continue
    for (const n of [m[1], m[2], m[3], m[4], m[5], ...(m[6]?.split(',') ?? [])]) if (n) names.add(n)
  }
  for (const lang of ['', 'ko/']) {
    const file = join(site, 'src', lang + page)
    if (!existsSync(file)) { missing.push(`${lang}${page}: missing`); continue }
    const text = readFileSync(file, 'utf8')
    for (const n of names) if (!text.includes(n)) missing.push(`${lang}${page}: ${n}`)
  }
}
if (missing.length) {
  console.error(`API pages miss what api/*.txt lists:\n  ${missing.join('\n  ')}`)
  process.exit(1)
}
mkdirSync(join(site, 'src/public'), { recursive: true })
for (const file of ['icon.svg', 'social.png']) copyFileSync(join(repo, 'docs/assets', file), join(site, 'src/public', file))
console.log(`prepared ${built.length} pages from the repository's docs`)
