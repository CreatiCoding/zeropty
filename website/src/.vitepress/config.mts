import { defineConfig, type DefaultTheme } from 'vitepress'
import { withMermaid } from 'vitepress-plugin-mermaid'
import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const src = fileURLToPath(new URL('..', import.meta.url))
// A page's own H1, so a sidebar entry never says something other than the page it opens.
function h1(link: string, fallback: string) {
  const file = `${src}${link.replace(/^\//, '').replace(/\/$/, '/index')}.md`
  if (!existsSync(file)) return fallback
  return readFileSync(file, 'utf8').match(/^# (.+)$/m)?.[1].replace(/<[^>]*>/g, '').trim() ?? fallback
}

const repo = 'https://github.com/CreatiCoding/zeropty'

// GitHub's heading ids, so the docs' own #anchors (written for GitHub) keep working here.
function slugify(text: string) {
  return text
    .trim()
    .toLowerCase()
    .replace(/<[^>]*>/g, '')
    .replace(/[^\p{L}\p{N}\p{M} _-]/gu, '')
    .replace(/ /g, '-')
}

// The pattern function is serialized into the client bundle, so it cannot use anything outside itself.
const edit = (text: string): DefaultTheme.EditLink => ({
  pattern: ({ frontmatter, filePath }) =>
    `https://github.com/CreatiCoding/zeropty/edit/main/${frontmatter.source ?? `website/src/${filePath}`}`,
  text,
})

type Words = Record<string, string>
const item = (p: string, t: Words, slug: string, path = `/guide/${slug}`) => ({ text: h1(p + path, t[slug]), link: p + path })

function guide(p: string, t: Words): DefaultTheme.SidebarItem[] {
  return [
    { text: t.start, items: [item(p, t, 'introduction'), item(p, t, 'getting-started'), item(p, t, 'install')] },
    {
      text: t.concepts,
      items: ['sessions', 'terminal', 'screens', 'interfaces'].map((s) => item(p, t, `c-${s}`, `/guide/concepts/${s}`)),
    },
    {
      text: t.guides,
      items: ['shell', 'terminal-programs', 'node', 'embed', 'plugins', 'moving', 'from-tmux'].map((s) => item(p, t, s)),
    },
    {
      text: t.project,
      items: ['benchmarks', 'design', 'plugins-internals', 'faq', 'versioning', 'releasing', 'changelog', 'contributing'].map((s) => item(p, t, s)),
    },
  ]
}

function reference(p: string, t: Words): DefaultTheme.SidebarItem[] {
  const r = (s: string) => ({ text: h1(`${p}/reference/${s}`, t[`r-${s}`]), link: `${p}/reference/${s}` })
  return [
    { text: t.index, link: `${p}/reference/` },
    { text: t.cli, items: [r('commands'), r('keys-and-formats'), r('environment'), r('tmux-compatibility')] },
    { text: t.programs, items: [r('socket-protocol'), r('c-abi'), r('node-api'), r('rust-api'), r('plugin-api')] },
  ]
}

const en: Words = {
  start: 'Getting started', concepts: 'Concepts', guides: 'Guides', project: 'Project',
  introduction: 'Introduction', 'getting-started': 'Get started', install: 'Install and platform support',
  'c-sessions': 'Sessions and the server', 'c-terminal': 'How a session gets a terminal',
  'c-screens': 'Screens, keys and waiting', 'c-interfaces': 'Four ways to drive zeropty',
  shell: 'Drive agents from the shell', 'terminal-programs': 'Run claude and terminal programs', node: 'Use it from Node.js',
  embed: 'Embed it in C or Rust', plugins: 'Write a plugin', moving: 'Move sessions and upgrade', 'from-tmux': 'Coming from tmux',
  benchmarks: 'Benchmarks', design: 'Design notes', 'plugins-internals': 'Plugin internals', faq: 'FAQ',
  versioning: 'Versioning', releasing: 'Releasing', contributing: 'Contributing',
  cli: 'Command line', programs: 'From programs',
  'r-commands': 'Commands', 'r-keys-and-formats': 'Keys, formats and exit codes', 'r-environment': 'Environment variables',
  'r-tmux-compatibility': 'tmux compatibility', 'r-socket-protocol': 'Socket protocol', 'r-c-abi': 'C ABI', 'r-node-api': 'Node.js API', index: 'Overview',
  'r-plugin-api': 'Plugin API',
}
const ko: Words = {
  start: '시작하기', concepts: '개념', guides: '가이드', project: '프로젝트',
  introduction: '소개', 'getting-started': '시작하기', install: '설치와 지원 환경',
  'c-sessions': '세션과 서버', 'c-terminal': '세션이 터미널을 얻는 방법',
  'c-screens': '화면, 키, 기다리기', 'c-interfaces': 'zeropty 를 다루는 네 가지 방법',
  shell: '셸에서 에이전트 다루기', 'terminal-programs': 'claude 와 터미널 프로그램 돌리기', node: 'Node.js 에서 쓰기',
  embed: 'C·Rust 에 넣기', plugins: '플러그인 만들기', moving: '세션 옮기기와 업그레이드', 'from-tmux': 'tmux 에서 넘어오기',
  benchmarks: '벤치마크', design: '설계 노트', 'plugins-internals': '플러그인 내부', faq: 'FAQ',
  versioning: '버전 정책', releasing: '릴리스', contributing: '기여하기',
  cli: '명령줄', programs: '프로그램에서',
  'r-commands': '명령', 'r-keys-and-formats': '키, 포맷, 종료 코드', 'r-environment': '환경변수',
  'r-tmux-compatibility': 'tmux 호환', 'r-socket-protocol': '소켓 프로토콜', 'r-c-abi': 'C ABI', 'r-node-api': 'Node.js API', index: '레퍼런스 안내',
  'r-plugin-api': '플러그인 API',
}

export default withMermaid(
  defineConfig({
    mermaid: { themeVariables: { fontSize: '15px', fontFamily: 'ui-sans-serif, system-ui, -apple-system, "Apple SD Gothic Neo", sans-serif' } },
    title: 'zeropty',
    description: 'The session engine for AI agents. tmux commands, no PTY, a fraction of the CPU and memory.',
    cleanUrls: true,
    appearance: 'dark',
    head: [
      ['link', { rel: 'icon', href: '/icon.svg', type: 'image/svg+xml' }],
      ['meta', { property: 'og:image', content: 'https://zeropty.pubdoc.dev/social.png' }],
      ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    ],
    markdown: {
      anchor: { slugify },
      theme: { light: 'github-light', dark: 'github-dark' },
    },
    // Each page's own description becomes its meta description.
    transformPageData(page) {
      if (page.frontmatter.description) page.description = page.frontmatter.description
    },
    themeConfig: {
      logo: '/icon.svg',
      socialLinks: [{ icon: 'github', link: repo }],
      search: {
        provider: 'local',
        options: {
          locales: {
            ko: {
              translations: {
                button: { buttonText: '검색', buttonAriaLabel: '검색' },
                modal: { noResultsText: '결과가 없어요', resetButtonTitle: '지우기', footer: { selectText: '선택', navigateText: '이동', closeText: '닫기' } },
              },
            },
          },
        },
      },
      outline: { level: [2, 3] },
    },
    locales: {
      root: {
        label: 'English',
        lang: 'en',
        themeConfig: {
          nav: [
            { text: 'Guide', link: '/guide/introduction', activeMatch: '/guide/(?!changelog)' },
            { text: 'Reference', link: '/reference/', activeMatch: '/reference/' },
            { text: 'Changelog', link: '/guide/changelog' },
          ],
          sidebar: { '/guide/': guide('', en), '/reference/': reference('', en) },
          editLink: edit('Edit this page on GitHub'),
          footer: { message: 'Released under the MIT or Apache-2.0 license.', copyright: 'zeropty' },
        },
      },
      ko: {
        label: '한국어',
        lang: 'ko',
        link: '/ko/',
        description: 'AI 에이전트를 위한 세션 엔진. tmux 명령 그대로, PTY 없이, CPU와 메모리는 훨씬 적게.',
        themeConfig: {
          nav: [
            { text: '가이드', link: '/ko/guide/introduction', activeMatch: '/ko/guide/(?!changelog)' },
            { text: '레퍼런스', link: '/ko/reference/', activeMatch: '/ko/reference/' },
            { text: '변경 기록', link: '/ko/guide/changelog' },
          ],
          sidebar: { '/ko/guide/': guide('/ko', ko), '/ko/reference/': reference('/ko', ko) },
          outline: { level: [2, 3], label: '이 페이지에서' },
          docFooter: { prev: '이전', next: '다음' },
          editLink: edit('GitHub에서 이 페이지 고치기'),
          footer: { message: 'MIT 또는 Apache-2.0 라이선스로 배포해요.', copyright: 'zeropty' },
          darkModeSwitchLabel: '테마',
          sidebarMenuLabel: '메뉴',
          returnToTopLabel: '맨 위로',
          langMenuLabel: '언어',
          notFound: { title: '페이지를 찾을 수 없어요', quote: '주소가 바뀌었거나 없는 페이지예요.', linkText: '처음으로' },
        },
      },
    },
  }),
)
