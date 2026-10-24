<script setup lang="ts">
import { useData, withBase } from 'vitepress'

// Every word on the page comes from the index.md front matter, so each language has its own.
const { frontmatter: fm } = useData()
</script>

<template>
  <div class="zp-home">
    <div class="zp-grid" aria-hidden="true" />
    <section class="zp-hero">
      <p class="zp-badge"><span class="zp-dot" />{{ fm.badge }}</p>
      <h1 class="zp-title">
        <span>{{ fm.headline[0] }}</span><br />
        <span class="zp-dim">{{ fm.headline[1] }}</span> <span>{{ fm.headline[2] }}</span>
      </h1>
      <p class="zp-tagline">{{ fm.tagline }}</p>
      <div class="zp-actions">
        <a class="zp-btn zp-primary" :href="withBase(fm.start.link)">{{ fm.start.text }} <span aria-hidden="true">→</span></a>
        <a class="zp-btn" :href="fm.github" target="_blank" rel="noopener">
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>
          GitHub
        </a>
      </div>

      <div class="zp-arch">
        <p class="zp-label">{{ fm.arch.label }}</p>
        <div class="zp-arch-row">
          <div class="zp-col">
            <p class="zp-label">{{ fm.arch.left }}</p>
            <div v-for="p in fm.arch.programs" :key="p" class="zp-chip"><span class="zp-dot" />{{ p }}</div>
          </div>
          <div class="zp-wire"><span class="zp-label">{{ fm.arch.in }}</span><i /></div>
          <div class="zp-core">
            <img :src="withBase('/icon.svg')" alt="" width="40" height="40" />
            <strong>{{ fm.arch.name }}</strong>
            <span>{{ fm.arch.core }}</span>
          </div>
          <div class="zp-wire"><span class="zp-label">{{ fm.arch.out }}</span><i /></div>
          <div class="zp-col">
            <p class="zp-label">{{ fm.arch.right }}</p>
            <div v-for="w in fm.arch.ways" :key="w[0]" class="zp-chip zp-split"><span>{{ w[0] }}</span><span class="zp-muted">{{ w[1] }}</span></div>
          </div>
        </div>
        <p v-if="fm.arch.more" class="zp-arch-more"><a :href="withBase(fm.arch.more.link)">{{ fm.arch.more.text }} →</a></p>
      </div>
    </section>

    <section v-if="fm.demo" class="zp-showcase">
      <p class="zp-label">{{ fm.demo.label }}</p>
      <h2 class="zp-h2">{{ fm.demo.title }}</h2>
      <figure class="zp-demo">
        <picture>
          <source media="(prefers-color-scheme: light)" :srcset="fm.demo.light" />
          <img :src="fm.demo.dark" :alt="fm.demo.alt" loading="lazy" />
        </picture>
        <figcaption>{{ fm.demo.caption }}</figcaption>
      </figure>
    </section>

    <section v-if="fm.showcase" class="zp-showcase">
      <p class="zp-label">{{ fm.showcase.label }}</p>
      <h2 class="zp-h2">{{ fm.showcase.title }}</h2>
      <div class="zp-shots">
        <figure v-for="s in fm.showcase.shots" :key="s.src" class="zp-phone">
          <img :src="withBase(s.src)" :alt="s.alt" loading="lazy" width="390" height="844" />
          <figcaption>{{ s.caption }}</figcaption>
        </figure>
      </div>
    </section>

    <section v-if="fm.stats" class="zp-stats">
      <div v-for="s in fm.stats" :key="s.label" class="zp-stat">
        <strong>{{ s.value }}</strong>
        <span>{{ s.label }}</span>
      </div>
    </section>
    <p v-if="fm.statsNote" class="zp-note"><a :href="withBase(fm.statsNote.link)">{{ fm.statsNote.text }}</a></p>

    <section class="zp-features">
      <a v-for="f in fm.features" :key="f.title" class="zp-card" :href="f.link ? withBase(f.link) : undefined">
        <p class="zp-label">{{ f.tag }}</p>
        <h3>{{ f.title }}</h3>
        <p>{{ f.body }}</p>
      </a>
    </section>

    <section v-if="fm.platforms" class="zp-platforms">
      <p class="zp-label">{{ fm.platforms.label }}</p>
      <div class="zp-plist">
        <span v-for="p in fm.platforms.items" :key="p.name" class="zp-chip">
          {{ p.name }}<em v-if="p.badge" class="zp-pbadge">{{ p.badge }}</em>
        </span>
      </div>
      <a class="zp-more" :href="withBase(fm.platforms.link)">{{ fm.platforms.text }} →</a>
    </section>

    <section class="zp-start">
      <p class="zp-label">{{ fm.quick.label }}</p>
      <pre><code><template v-for="(l, i) in fm.quick.lines" :key="i"><span class="zp-prompt">$ </span>{{ l }}
</template></code></pre>
      <a class="zp-btn zp-primary" :href="withBase(fm.quick.link)">{{ fm.quick.text }} <span aria-hidden="true">→</span></a>
    </section>

    <nav v-if="fm.links" class="zp-links" :aria-label="fm.links.label">
      <a v-for="l in fm.links.items" :key="l.link" :href="l.link.startsWith('http') ? l.link : withBase(l.link)">{{ l.text }}</a>
    </nav>
  </div>
</template>
