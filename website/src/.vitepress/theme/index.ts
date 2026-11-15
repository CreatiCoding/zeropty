import DefaultTheme from 'vitepress/theme'
import { inBrowser, useData, type Theme } from 'vitepress'
import { nextTick, watch } from 'vue'
import Home from './Home.vue'
import './style.css'

// The status bar colour (theme-color) follows the site's own theme switch (.dark), not only the device's (AGENTS.md).
function syncThemeColor() {
  const bg = getComputedStyle(document.documentElement).getPropertyValue('--vp-nav-bg-color').trim()
  if (bg) for (const m of document.querySelectorAll('meta[name="theme-color"]')) m.setAttribute('content', bg)
}

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('Home', Home)
  },
  setup() {
    if (!inBrowser) return
    const { isDark } = useData()
    watch(isDark, () => nextTick(syncThemeColor), { immediate: true })
  },
} satisfies Theme
