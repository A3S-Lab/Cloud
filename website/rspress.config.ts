import path from 'node:path';
import { defineConfig } from '@rspress/core';

const base = process.env.DOCS_BASE ?? '/Cloud/';
const siteOrigin = process.env.DOCS_ORIGIN ?? 'https://a3s-lab.github.io';

export default defineConfig({
  root: path.join(__dirname, 'docs'),
  base,
  siteOrigin,
  title: 'A3S Cloud',
  description:
    'A self-hosted desired-state control plane for durable A3S workloads, delivery, reachability, and operations.',
  lang: 'en',
  icon: '/favicon.svg',
  logo: '/a3s-cloud-mark.svg',
  logoText: 'A3S Cloud',
  outDir: 'doc_build',
  head: [
    ['meta', { name: 'theme-color', content: '#050807' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'A3S Cloud' }],
    [
      'meta',
      {
        property: 'og:image',
        content: `${siteOrigin}${base}social-card.svg`,
      },
    ],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],
    (route) => [
      'link',
      {
        rel: 'canonical',
        href: `${siteOrigin}${base.replace(/\/$/, '')}${route.routePath}`,
      },
    ],
  ],
  themeConfig: {
    darkMode: 'force-dark',
    enableContentAnimation: true,
    nav: [
      { text: 'Control loop', link: '/#control-loop' },
      { text: 'Capabilities', link: '/#capabilities' },
      { text: 'Roadmap', link: '/#roadmap' },
      { text: 'Architecture', link: '/architecture/' },
      { text: 'Docs', link: '/docs/' },
    ],
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/A3S-Lab/Cloud',
      },
    ],
  },
});
