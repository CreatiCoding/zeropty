// Draws README's images (logo, benchmark chart, how-it-fits diagram) as SVG, a light and a dark one each,
// in Korean (README.ko.md: bench-light.svg, ...) and in English (README.md: bench-en-light.svg, ...).
// The numbers come from docs/BENCHMARKS.md: when those change, edit BENCH below and run
//   node docs/assets/make.mjs
// Every number printed is the measured range exactly as docs/BENCHMARKS.md and README state it (never
// rounded, never a single value picked from a range); only a bar's length uses the middle of its range.
import { writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const SANS = `-apple-system, BlinkMacSystemFont, 'Segoe UI', 'Apple SD Gothic Neo', 'Noto Sans KR', Helvetica, Arial, sans-serif`
const MONO = `ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, monospace`

const THEMES = {
  light: { bg: '#ffffff', panel: '#f6f8fa', line: '#d0d7de', text: '#1f2328', sub: '#59636e', bar: '#c4ccd4', barText: '#59636e', a: '#0891b2', b: '#7c3aed', badgeBg: '#ecfeff', badgeText: '#0e7490' },
  dark: { bg: '#0d1117', panel: '#161b22', line: '#30363d', text: '#e6edf3', sub: '#9198a1', bar: '#3d444d', barText: '#9198a1', a: '#22d3ee', b: '#a78bfa', badgeBg: '#0b2b33', badgeText: '#67e8f9' },
}

// Every word drawn, per language. The numbers stay in BENCH, shared by both.
const TEXT = {
  ko: {
    suffix: '', labelW: 132,
    cpu: 'CPU', mem: '메모리', latency: '응답 속도',
    agents: '에이전트 세션 400개 (실제 claude 작업 재생) · 낮을수록 좋음',
    sendKeys: '프로그램에서 send-keys 한 번 (Node, p50, tmux는 명령마다 실행) · 낮을수록 좋음',
    server: 'zeropty 서버', embedded: 'zeropty 임베디드', socket: 'zeropty 소켓',
    vs: (s) => `tmux ${s} 대비`, better: '더 좋음',
    footer: '숫자: 측정 범위 · 배수: 범위 양끝으로 나눠 버림 · 막대: 범위의 가운데 · Apple M1, tmux 3.7c, 번갈아 측정 · 전체: docs/BENCHMARKS.md',
    tagline: 'AI 에이전트를 위한 세션 엔진',
    app: '에이전트 앱', anyLang: 'Node · Python · Rust · C — 무엇이든',
    engine: 'zeropty 엔진 (임베디드)', call: '함수 호출 한 번 0.04~0.06ms (Node)', noServer: '서버 프로세스 없음 · PTY 없음',
    socketPlugin: '+ 소켓 플러그인 (선택)', listenLine: 'listen("app.sock") 한 줄',
    sessions: '× 1000 세션', pipes: '파이프로 연결 · 커널 PTY 한도 없음',
    people: ['사람·', '스크립트'],
    socialSub: '에이전트 1000개를 띄우고, 읽고, 키를 보내고, 끝날 때까지 기다리기',
    socialStats: ['CPU (tmux 대비)', '메모리 (tmux 대비)', 'send-keys (임베디드)', '세션 · PTY 한도 없음'],
  },
  en: {
    suffix: '-en', labelW: 156,
    cpu: 'CPU', mem: 'Memory', latency: 'Latency',
    agents: '400 agent sessions (replaying real claude work) · lower is better',
    sendKeys: 'one send-keys from a program (Node, p50; tmux runs per command) · lower is better',
    server: 'zeropty server', embedded: 'zeropty embedded', socket: 'zeropty socket',
    vs: (s) => `vs tmux ${s}`, better: 'better',
    footer: 'Ratios: range ends divided, rounded down · bars: range midpoint · Apple M1, tmux 3.7c, interleaved · all data: docs/BENCHMARKS.md',
    tagline: 'The session engine for AI agents',
    app: 'Agent app', anyLang: 'Node · Python · Rust · C — anything',
    engine: 'zeropty engine (embedded)', call: 'one function call 0.04~0.06ms (Node)', noServer: 'no server process · no PTY',
    socketPlugin: '+ socket plugin (optional)', listenLine: 'one line: listen("app.sock")',
    sessions: '× 1000 sessions', pipes: 'connected by pipes · no kernel PTY limit',
    people: ['People,', 'scripts'],
    socialSub: 'Run 1000 agents: start, read, send keys, wait until they finish',
    socialStats: ['CPU (vs tmux)', 'memory (vs tmux)', 'send-keys (embedded)', 'sessions · no PTY limit'],
  },
}

const benchRows = (L) => [
  {
    title: L.cpu, note: L.agents,
    rows: [
      { name: 'tmux', v: 51.5, label: '51.5%' },
      { name: 'zeropty', v: 3.62, label: '3.55~3.69%', x: '13.9~14.5×', us: true },
    ],
  },
  {
    title: L.mem, note: L.agents,
    rows: [
      { name: 'tmux', v: 31.9, label: '31.9 MB' },
      { name: L.server, v: 2.68, label: '2.64~2.72 MB', x: '11.7~12.0×', us: true },
      { name: L.embedded, v: 1.82, label: '1.82 MB', x: '17.0×', us: true, vs: L.vs('31.1 MB') },
    ],
  },
  {
    title: L.latency, note: L.sendKeys,
    rows: [
      { name: 'tmux', v: 8.4, label: '6.43~10.37 ms' },
      { name: L.socket, v: 0.23, label: '0.22~0.24 ms', x: '26.7~47.1×', us: true },
      { name: L.embedded, v: 0.05, label: '0.04~0.06 ms', x: '107~259×', us: true },
    ],
  },
]

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

function gradient(id, t) {
  return `<linearGradient id="${id}" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="${t.a}"/><stop offset="1" stop-color="${t.b}"/></linearGradient>`
}

// ---------------------------------------------------------------- benchmark chart

function bench(t, L) {
  const W = 880, pad = 32, labelW = L.labelW, barX = pad + labelW, barMax = 540 - (labelW - 132), rowH = 34, gap = 10
  let y = 40
  let body = ''
  for (const p of benchRows(L)) {
    const h = 64 + p.rows.length * (rowH + gap)
    body += `<rect x="${pad - 12}" y="${y - 30}" width="${W - 2 * pad + 24}" height="${h}" rx="14" fill="${t.panel}" stroke="${t.line}"/>`
    body += `<text x="${pad + 4}" y="${y}" font-family="${SANS}" font-size="19" font-weight="700" fill="${t.text}">${esc(p.title)}</text>`
    body += `<text x="${pad + 130}" y="${y}" font-family="${SANS}" font-size="13" fill="${t.sub}">${esc(p.note)}</text>`
    y += 22
    const max = Math.max(...p.rows.map((r) => r.v))
    for (const r of p.rows) {
      const w = Math.max(4, (r.v / max) * barMax)
      const cy = y + rowH / 2
      body += `<text x="${barX - 14}" y="${cy + 5}" text-anchor="end" font-family="${MONO}" font-size="14" font-weight="${r.us ? 700 : 500}" fill="${r.us ? t.text : t.sub}">${esc(r.name)}</text>`
      body += `<rect x="${barX}" y="${y + 5}" width="${w.toFixed(1)}" height="${rowH - 10}" rx="6" fill="${r.us ? 'url(#g)' : t.bar}"/>`
      const vx = barX + w + 10
      body += `<text x="${vx.toFixed(1)}" y="${cy + 5}" font-family="${MONO}" font-size="14" font-weight="${r.us ? 700 : 500}" fill="${r.us ? t.text : t.barText}">${esc(r.label)}</text>`
      if (r.x) {
        const bx = vx + r.label.length * 8.6 + 12, bw = r.x.length * 8.5 + 64
        body += `<rect x="${bx.toFixed(1)}" y="${y + 6}" width="${bw}" height="${rowH - 12}" rx="11" fill="${t.badgeBg}"/>`
        body += `<text x="${(bx + bw / 2).toFixed(1)}" y="${cy + 5}" text-anchor="middle" font-family="${SANS}" font-size="13" font-weight="700" fill="${t.badgeText}">${esc(r.x)} ${esc(L.better)}</text>`
        // A ratio measured against a different tmux run than the panel's tmux bar says which one.
        if (r.vs) body += `<text x="${(bx + bw + 10).toFixed(1)}" y="${cy + 5}" font-family="${SANS}" font-size="12" fill="${t.sub}">${esc(r.vs)}</text>`
      }
      y += rowH + gap
    }
    y += 58
  }
  y -= 26
  body += `<text x="${pad}" y="${y}" font-family="${SANS}" font-size="12" fill="${t.sub}">${esc(L.footer)}</text>`
  const H = y + 22
  return svg(W, H, t, `<defs>${gradient('g', t)}</defs>${body}`)
}

// For the badge text width above, the word "better" is drawn inside the badge: widen it.
function svg(w, h, t, inner, bg = true) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${bg ? `<rect width="100%" height="100%" rx="16" fill="${t.bg}"/>` : ''}${inner}</svg>\n`
}

// ---------------------------------------------------------------- logo

function mark(t, size = 128, id = 'm') {
  // A dark tile, a field of sessions (a few lit: busy agents), and an agent's prompt.
  const s = size / 128
  let cells = ''
  const lit = new Set(['1,0', '4,1', '2,3', '5,4', '0,5', '3,5'])
  for (let i = 0; i < 6; i++) for (let j = 0; j < 6; j++) {
    const on = lit.has(`${i},${j}`)
    cells += `<rect x="${(58 + i * 10.5) * s}" y="${(16 + j * 10.5) * s}" width="${7 * s}" height="${7 * s}" rx="${1.8 * s}" fill="${on ? `url(#${id}a)` : '#ffffff'}" opacity="${on ? 1 : 0.13}"/>`
  }
  return `<defs><linearGradient id="${id}bg" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#111827"/><stop offset="1" stop-color="#1e1b4b"/></linearGradient>${gradient(id + 'a', { a: '#22d3ee', b: '#a78bfa' })}</defs>`
    + `<rect width="${size}" height="${size}" rx="${28 * s}" fill="url(#${id}bg)"/>`
    + `<rect x="${0.75 * s}" y="${0.75 * s}" width="${size - 1.5 * s}" height="${size - 1.5 * s}" rx="${27.5 * s}" fill="none" stroke="#ffffff" stroke-opacity="0.12" stroke-width="${1.5 * s}"/>`
    + cells
    + `<path d="M${22 * s} ${74 * s} L${44 * s} ${91 * s} L${22 * s} ${108 * s}" fill="none" stroke="url(#${id}a)" stroke-width="${10 * s}" stroke-linecap="round" stroke-linejoin="round"/>`
    + `<rect x="${56 * s}" y="${100 * s}" width="${44 * s}" height="${10 * s}" rx="${5 * s}" fill="url(#${id}a)"/>`
}

function icon() { return `<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">${mark(null)}</svg>\n` }

function logo(t, L) {
  const W = 520, H = 128
  const inner = `<g>${mark(t, 112, 'l')}</g>`
    + `<text x="140" y="74" font-family="${MONO}" font-size="66" font-weight="800" letter-spacing="-2" fill="${t.text}">zeropty</text>`
    + `<text x="143" y="104" font-family="${SANS}" font-size="17" font-weight="500" fill="${t.sub}">${esc(L.tagline)}</text>`
  return svg(W, H, t, `<g transform="translate(0,8)">${inner}</g>`, false)
}

// ---------------------------------------------------------------- how it fits

function how(t, L) {
  const W = 880, H = 362
  const box = (x, y, w, h, extra = '') => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="14" fill="${t.panel}" stroke="${t.line}" ${extra}/>`
  const txt = (x, y, s, size = 14, weight = 500, fill = t.text, font = SANS, anchor = 'start') => `<text x="${x}" y="${y}" text-anchor="${anchor}" font-family="${font}" font-size="${size}" font-weight="${weight}" fill="${fill}">${esc(s)}</text>`
  let g = `<defs>${gradient('h', t)}<marker id="arr" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 L10 5 L0 10z" fill="${t.sub}"/></marker></defs>`
  // The agent app with the engine inside.
  g += box(24, 24, 330, 314)
  g += txt(44, 56, L.app, 17, 700) + txt(44, 78, L.anyLang, 13, 400, t.sub)
  g += `<rect x="44" y="100" width="290" height="118" rx="12" fill="url(#h)" opacity="0.14"/><rect x="44" y="100" width="290" height="118" rx="12" fill="none" stroke="url(#h)" stroke-width="2"/>`
  g += txt(64, 130, L.engine, 16, 700) + txt(64, 154, 'send-keys · capture · wait-pane', 12.5, 500, t.sub, MONO)
  g += txt(64, 176, L.call, 13, 400, t.sub) + txt(64, 198, L.noServer, 13, 400, t.sub)
  g += `<rect x="44" y="262" width="290" height="56" rx="12" fill="none" stroke="${t.line}" stroke-dasharray="5 5"/>`
  g += txt(64, 286, L.socketPlugin, 14, 600) + txt(64, 306, L.listenLine, 12, 400, t.sub, MONO)
  // Sessions.
  const names = ['claude', 'codex', 'claude', 'bash', 'claude', 'python']
  g += `<path d="M334 159 C 400 159, 400 96, 452 84" fill="none" stroke="${t.sub}" stroke-width="1.6" marker-end="url(#arr)"/>`
  g += `<path d="M334 159 C 400 159, 400 196, 452 184" fill="none" stroke="${t.sub}" stroke-width="1.6" marker-end="url(#arr)"/>`
  for (let k = 0; k < 6; k++) {
    const x = 462 + (k % 3) * 104, y = 40 + Math.floor(k / 3) * 100
    g += `<rect x="${x}" y="${y}" width="92" height="88" rx="10" fill="${t.bg}" stroke="${t.line}"/>`
    g += `<circle cx="${x + 13}" cy="${y + 13}" r="3.2" fill="${k % 2 ? t.b : t.a}"/>` + txt(x + 22, y + 17, names[k], 11, 600, t.sub, MONO)
    for (let l = 0; l < 4; l++) g += `<rect x="${x + 11}" y="${y + 30 + l * 13}" width="${[64, 48, 58, 30][(l + k) % 4]}" height="5" rx="2.5" fill="${t.line}"/>`
  }
  g += txt(616, 262, L.sessions, 18, 800, t.text, SANS, 'middle')
  g += txt(616, 284, L.pipes, 12, 400, t.sub, SANS, 'middle')
  // Humans and tools through the plugin.
  g += box(786, 240, 74, 98)
  g += txt(823, 268, L.people[0], 12, 600, t.text, SANS, 'middle') + txt(823, 284, L.people[1], 12, 600, t.text, SANS, 'middle')
  g += txt(823, 306, 'zeropty', 11, 600, t.sub, MONO, 'middle') + txt(823, 322, 'attach', 11, 600, t.sub, MONO, 'middle')
  g += `<path d="M786 312 C 640 330, 460 322, 336 296" fill="none" stroke="${t.sub}" stroke-width="1.4" stroke-dasharray="4 4" marker-end="url(#arr)"/>`
  return svg(W, H, t, g)
}

// ---------------------------------------------------------------- social card (GitHub's link preview)

function social(L) {
  const t = THEMES.dark, W = 1280, H = 640
  const stat = (x, big, small) => `<text x="${x}" y="512" font-family="${SANS}" font-size="46" font-weight="800" fill="url(#sa)">${esc(big)}</text>`
    + `<text x="${x}" y="548" font-family="${SANS}" font-size="22" font-weight="500" fill="${t.sub}">${esc(small)}</text>`
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">`
    + `<defs><radialGradient id="glow" cx="0.15" cy="0.2" r="0.9"><stop offset="0" stop-color="#1e1b4b"/><stop offset="1" stop-color="#0d1117"/></radialGradient>${gradient('sa', t)}</defs>`
    + `<rect width="${W}" height="${H}" fill="url(#glow)"/>`
    + `<g transform="translate(96,104)">${mark(t, 200, 's')}</g>`
    + `<text x="340" y="196" font-family="${MONO}" font-size="128" font-weight="800" letter-spacing="-4" fill="${t.text}">zeropty</text>`
    + `<text x="346" y="262" font-family="${SANS}" font-size="40" font-weight="600" fill="${t.text}">${esc(L.tagline)}</text>`
    + `<text x="346" y="310" font-family="${SANS}" font-size="26" font-weight="400" fill="${t.sub}">${esc(L.socialSub)}</text>`
    + `<line x1="96" y1="420" x2="${W - 96}" y2="420" stroke="${t.line}"/>`
    + stat(96, '13.9~14.5×', L.socialStats[0]) + stat(416, '11.7~12.0×', L.socialStats[1]) + stat(736, '107~259×', L.socialStats[2]) + stat(1036, '1000', L.socialStats[3])
    + `</svg>\n`
}

for (const L of Object.values(TEXT)) {
  for (const [name, t] of Object.entries(THEMES)) {
    writeFileSync(join(here, `bench${L.suffix}-${name}.svg`), bench(t, L))
    writeFileSync(join(here, `logo${L.suffix}-${name}.svg`), logo(t, L))
    writeFileSync(join(here, `how${L.suffix}-${name}.svg`), how(t, L))
  }
  writeFileSync(join(here, `social${L.suffix}.svg`), social(L))
}
writeFileSync(join(here, 'icon.svg'), icon())
console.log('wrote', here)
