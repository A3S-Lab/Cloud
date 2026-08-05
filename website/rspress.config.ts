import path from 'node:path';
import { defineConfig } from '@rspress/core';

const base = process.env.DOCS_BASE ?? '/Cloud/';
const siteOrigin = process.env.DOCS_ORIGIN ?? 'https://a3s-lab.github.io';

function includeSimplifiedChinese(
  source: Record<string, Record<string, string>>,
) {
  return Object.fromEntries(
    Object.entries(source).map(([key, translations]) => [
      key,
      {
        ...translations,
        'zh-CN': translations.zh ?? translations.en,
      },
    ]),
  );
}

export default defineConfig({
  root: path.join(__dirname, 'docs'),
  base,
  siteOrigin,
  title: 'A3S OS',
  description:
    'A3S OS 企业级 AI 操作系统，提供自主工作流编排、异构智能体工厂、安全监控中台与 A3S Web 客户端。',
  lang: 'zh-CN',
  i18nSource: includeSimplifiedChinese,
  icon: '/favicon.svg',
  logo: '/a3s-cloud-mark.svg',
  logoText: 'A3S OS',
  outDir: 'doc_build',
  head: [
    ['meta', { name: 'theme-color', content: '#ffffff' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'A3S OS' }],
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
    darkMode: 'force-light',
    enableContentAnimation: true,
    nav: [
      { text: 'Workflow', link: '/#workflow' },
      { text: 'Agent Factory', link: '/#agent-factory' },
      { text: '安全监控', link: '/#security-operations' },
      { text: 'A3S Web', link: '/#web-client' },
      { text: '模块架构', link: '/#architecture' },
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
