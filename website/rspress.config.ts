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
    'A3S OS 基于云原生微服务架构，打造可管、可控、可协作、可审计的企业级 AI Native 操作系统，提供全栈国产化的端云一体智能体和工作流安全执行平台。',
  lang: 'zh-CN',
  i18nSource: includeSimplifiedChinese,
  icon: '/a3s-os-logo.png',
  logo: '/a3s-os-logo.png',
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
      { text: '端云底座', link: '/#edge-cloud-foundation' },
      {
        text: '产品',
        items: [
          { text: '工作流编排', link: '/#workflow' },
          { text: '智能体工厂', link: '/#agent-factory' },
          { text: '统一网关', link: '/#unified-gateway' },
        ],
      },
      { text: '端侧智能体', link: '/#edge-agent' },
      { text: '行业方案', link: '/#solutions' },
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
