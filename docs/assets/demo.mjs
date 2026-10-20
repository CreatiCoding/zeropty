// Records README's terminal demo from a real run of its quickstart and renders it to an animated WebP,
// a light and a dark one, in Korean (README.ko.md: demo-light.webp) and English (README.md:
// demo-en-light.webp): what the frames show is what the commands printed, and the wait-pane badge is
// the time that run measured (one run for both languages). Run after ./build.sh (uses dist/zeropty, on
// a socket of its own); DEMO_LANGS="en" renders one language only (default: "ko en"):
//   node docs/assets/demo.mjs
// Needs Google Chrome (renders the frames) and img2webp (brew install webp). README loads the WebPs from
// https://cdn.creco.dev/zeropty/ (the GitHub mobile app shows repository images as broken): upload
// the new demo-*.webp files after rerunning. DEMO_PEEK=<dir> also keeps the last frames as PNG there.
import { execFileSync, spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const dist = join(here, '..', '..', 'dist')
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
if (!existsSync(join(dist, 'zeropty'))) throw new Error('run ./build.sh first')

const MONO = `ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace`
const SANS = `-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Apple SD Gothic Neo', 'Noto Sans KR', Helvetica, Arial, sans-serif`
const THEMES = {
  light: { bg: '#ffffff', win: '#f6f8fa', bar: '#eaeef2', line: '#d0d7de', text: '#1f2328', sub: '#59636e', prompt: '#0891b2', badgeBg: '#ecfeff', badgeText: '#0e7490' },
  dark: { bg: '#0d1117', win: '#161b22', bar: '#1f252d', line: '#30363d', text: '#e6edf3', sub: '#9198a1', prompt: '#22d3ee', badgeBg: '#0b2b33', badgeText: '#67e8f9' },
}

// The quickstart of README.md, as typed.
const STEPS = [
  `zeropty new-session -d -s demo -x 80 -y 6 sh`,
  `zeropty send-keys -t demo -l 'sleep 1; echo "done: $((6 * 7))"'`,
  `zeropty send-keys -t demo Enter`,
  `zeropty wait-pane -t demo -m 'done: 42' -T 10000`,
  `zeropty capture-pane -p -t demo`,
  `zeropty kill-server`,
]
const WAIT = 3

// 1. Run it for real, on a private socket, and keep what each command printed and how long it took.
const tmp = mkdtempSync(join(tmpdir(), 'zpdemo-'))
const env = { ...process.env, PATH: `${dist}:${process.env.PATH}`, ZEROPTY_SOCK: join(tmp, 's.sock') }
const run = STEPS.map(cmd => {
  const t0 = performance.now()
  let out
  try {
    out = execFileSync('/bin/sh', ['-c', `${cmd} 2>&1`], { env, encoding: 'utf8' })
  } catch (e) {
    spawnSync(join(dist, 'zeropty'), ['kill-server'], { env, stdio: 'ignore' })
    rmSync(tmp, { recursive: true, force: true })
    throw new Error(`${cmd}: ${e.stdout}`)
  }
  return { cmd, out: out.replace(/\n+$/, ''), ms: performance.now() - t0 }
})
if (!run[WAIT + 1].out.includes('done: 42')) throw new Error(`unexpected capture: ${run[WAIT + 1].out}`)
// The words drawn, per language (the terminal text is what the commands printed).
const TEXT = {
  ko: { suffix: '', title: 'zeropty · 1분 시작', badge: (s) => `${s}초 · 폴링 없이, 글자가 뜬 순간` },
  en: { suffix: '-en', title: 'zeropty · quick start', badge: (s) => `${s}s · no polling, the moment it appeared` },
}
const seconds = (run[WAIT].ms / 1000).toFixed(2)
for (const lang of (process.env.DEMO_LANGS || 'ko en').split(/\s+/)) if (!TEXT[lang]) throw new Error(`DEMO_LANGS: no language ${lang}`)

// 2. Frames: typing, Enter, the output; the wait shows its cursor for as long as the wait took.
function framesFor(L) {
const frames = [] // { lines: [{ prompt?, text, badge? }], cursor, ms }
const done = []
const frame = (lines, ms) => frames.push({ lines: lines.map(l => ({ ...l })), ms: Math.round(ms) })
frame([{ prompt: true, text: '', cursor: true }], 700)
run.forEach((r, i) => {
  for (let n = 4; n < r.cmd.length; n += 4) frame([...done, { prompt: true, text: r.cmd.slice(0, n), cursor: true }], 45)
  frame([...done, { prompt: true, text: r.cmd, cursor: true }], 350)
  done.push({ prompt: true, text: r.cmd })
  if (i === WAIT) frame([...done, { text: '', cursor: true }], r.ms)
  if (i === WAIT) done[done.length - 1].badge = L.badge(seconds)
  for (const l of r.out ? r.out.split('\n') : []) done.push({ text: l })
  frame([...done, { prompt: true, text: '', cursor: true }], i === run.length - 1 ? 3500 : i === WAIT ? 900 : 300)
})
return frames
}

const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
const W = 880, LH = 24, FS = 14.5, CW = FS * 0.602, X0 = 28, Y0 = 72
const H = Y0 + LH * Math.max(...framesFor(TEXT.ko).map(f => f.lines.length)) + 20

function svg(f, t, L) {
  let g = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">`
  g += `<rect width="100%" height="100%" fill="${t.bg}"/>`
  g += `<rect x="8" y="8" width="${W - 16}" height="${H - 16}" rx="12" fill="${t.win}" stroke="${t.line}"/>`
  g += `<path d="M8 20 a12 12 0 0 1 12 -12 h${W - 40} a12 12 0 0 1 12 12 v18 h-${W - 16}z" fill="${t.bar}"/>`
  ;['#ff5f57', '#febc2e', '#28c840'].forEach((c, i) => (g += `<circle cx="${30 + i * 20}" cy="23" r="6" fill="${c}"/>`))
  g += `<text x="${W / 2}" y="28" text-anchor="middle" font-family="${SANS}" font-size="13" font-weight="600" fill="${t.sub}">${esc(L.title)}</text>`
  f.lines.forEach((l, i) => {
    const y = Y0 + i * LH
    let x = X0
    if (l.prompt) {
      g += `<text x="${x}" y="${y}" font-family="${MONO}" font-size="${FS}" font-weight="700" fill="${t.prompt}">$</text>`
      x += CW * 2
    }
    if (l.text) g += `<text x="${x}" y="${y}" xml:space="preserve" font-family="${MONO}" font-size="${FS}" fill="${l.prompt ? t.text : t.sub}">${esc(l.text)}</text>`
    const end = x + CW * l.text.length
    if (l.cursor) g += `<rect x="${end + 1}" y="${y - FS + 2}" width="${CW}" height="${FS + 3}" fill="${t.prompt}" opacity="0.85"/>`
    if (l.badge) {
      const bw = l.badge.length * 7.6 + 24
      g += `<rect x="${end + 16}" y="${y - 16}" width="${bw}" height="22" rx="11" fill="${t.badgeBg}" stroke="${t.prompt}" stroke-opacity="0.5"/>`
      g += `<text x="${end + 16 + bw / 2}" y="${y - 1}" text-anchor="middle" font-family="${SANS}" font-size="12.5" font-weight="700" fill="${t.badgeText}">${esc(l.badge)}</text>`
    }
  })
  return g + '</svg>'
}

// 3. Render each frame with Chrome at 1x, then put them together.
const wrote = []
try {
  for (const lang of (process.env.DEMO_LANGS || 'ko en').split(/\s+/)) {
  const L = TEXT[lang], frames = framesFor(L)
  for (const [name, t] of Object.entries(THEMES)) {
    const args = ['-loop', '0', '-lossy', '-q', '80', '-m', '6']
    frames.forEach((f, i) => {
      const base = join(tmp, `${lang}-${name}-${String(i).padStart(3, '0')}`)
      writeFileSync(`${base}.svg`, svg(f, t, L))
      writeFileSync(`${base}.html`, `<html><body style="margin:0;background:${t.bg}"><img src="file://${base}.svg" width="${W}" height="${H}" style="display:block"></body></html>`)
      execFileSync(CHROME, ['--headless=new', '--disable-gpu', `--user-data-dir=${join(tmp, 'prof')}`, '--hide-scrollbars',
        '--force-device-scale-factor=1', `--window-size=${W},${H}`, `--screenshot=${base}.png`, `file://${base}.html`], { stdio: 'ignore' })
      args.push('-d', String(f.ms), `${base}.png`)
    })
    if (process.env.DEMO_PEEK) copyFileSync(join(tmp, `${lang}-${name}-${String(frames.length - 1).padStart(3, '0')}.png`), join(process.env.DEMO_PEEK, `demo${L.suffix}-${name}-last.png`))
    execFileSync('img2webp', [...args, '-o', join(here, `demo${L.suffix}-${name}.webp`)], { stdio: 'inherit' })
    wrote.push(`demo${L.suffix}-${name}.webp`)
  }
  }
  console.log(`wrote ${wrote.join(' ')} (wait-pane ${seconds}s)`)
} finally {
  rmSync(tmp, { recursive: true, force: true })
}
