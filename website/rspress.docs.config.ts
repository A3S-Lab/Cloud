import { readFileSync } from 'node:fs';
import path from 'node:path';
import { defineConfig } from '@rspress/core';

type DocumentationLanguage = 'en' | 'zh';

type VersionRegistry = {
  default: string;
  defaultLanguage: DocumentationLanguage;
  languages: Array<{
    id: DocumentationLanguage;
    label: string;
  }>;
  versions: string[];
};

const productBase = normalizeBase(process.env.DOCS_BASE ?? '/Cloud/');
const docsBase = `${productBase}docs/`;
const siteOrigin = process.env.DOCS_ORIGIN ?? 'https://a3s-lab.github.io';
const registry = loadVersionRegistry();
const defaultLanguage = registry.defaultLanguage;

const copy = {
  zh: {
    description: 'A3S OS 的版本化产品文档，涵盖控制循环、产品边界和运维契约。',
    docs: '文档',
    home: '产品首页',
    overview: '概览',
    title: 'A3S OS 文档',
    versioning: '版本管理',
  },
  en: {
    description:
      'Versioned A3S OS product documentation covering the control loop, product boundaries, and operating contracts.',
    docs: 'Docs',
    home: 'Product home',
    overview: 'Overview',
    title: 'A3S OS Docs',
    versioning: 'Versioning',
  },
} as const;

function normalizeBase(value: string) {
  return `/${value.replace(/^\/+|\/+$/g, '')}/`;
}

function isDocumentationLanguage(
  value: unknown,
): value is DocumentationLanguage {
  return value === 'zh' || value === 'en';
}

function loadVersionRegistry(): VersionRegistry {
  const registryPath = path.join(__dirname, 'documentation', 'versions.json');
  const value: unknown = JSON.parse(readFileSync(registryPath, 'utf8'));
  if (
    typeof value !== 'object' ||
    value === null ||
    !('default' in value) ||
    typeof value.default !== 'string' ||
    !('defaultLanguage' in value) ||
    !isDocumentationLanguage(value.defaultLanguage) ||
    !('languages' in value) ||
    !Array.isArray(value.languages) ||
    !value.languages.every(
      (language) =>
        typeof language === 'object' &&
        language !== null &&
        'id' in language &&
        isDocumentationLanguage(language.id) &&
        'label' in language &&
        typeof language.label === 'string',
    ) ||
    !('versions' in value) ||
    !Array.isArray(value.versions) ||
    !value.versions.every((version) => typeof version === 'string')
  ) {
    throw new Error('documentation/versions.json is not a version registry');
  }
  return {
    default: value.default,
    defaultLanguage: value.defaultLanguage,
    languages: value.languages,
    versions: value.versions,
  };
}

function docsRoute(
  version: string,
  language: DocumentationLanguage,
  page = '',
) {
  const parts = [
    version === registry.default ? '' : version,
    language === defaultLanguage ? '' : language,
    page,
  ].filter(Boolean);
  if (parts.length === 0) return '/';
  return `/${parts.join('/')}/`;
}

function deployedRoutePath(routePath: string) {
  return `${routePath.replace(/\/$/, '')}/`;
}

function navFor(language: DocumentationLanguage) {
  return Object.fromEntries(
    registry.versions.map((version) => [
      version,
      [
        {
          text: copy[language].home,
          link: `${siteOrigin}${productBase}`,
        },
        {
          text: copy[language].docs,
          link: docsRoute(version, language),
        },
        {
          text: copy[language].versioning,
          link: docsRoute(version, language, 'versioning'),
        },
      ],
    ]),
  );
}

function sidebarFor(language: DocumentationLanguage) {
  return Object.fromEntries(
    registry.versions.map((version) => {
      const root = docsRoute(version, language);
      return [
        root,
        [
          {
            text: copy[language].docs,
            items: [
              {
                text: copy[language].overview,
                link: root,
              },
              {
                text: copy[language].versioning,
                link: docsRoute(version, language, 'versioning'),
              },
            ],
          },
        ],
      ];
    }),
  );
}

export default defineConfig({
  root: path.join(__dirname, 'documentation'),
  base: docsBase,
  siteOrigin,
  title: 'A3S OS Docs',
  description: copy.zh.description,
  lang: defaultLanguage,
  icon: new URL('./docs/public/favicon.svg', import.meta.url),
  logo: '/a3s-cloud-mark.svg',
  logoText: 'A3S OS Docs',
  outDir: 'docs_build',
  multiVersion: registry,
  route: {
    cleanUrls: true,
  },
  head: [
    ['meta', { name: 'theme-color', content: '#ffffff' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'A3S OS Docs' }],
    (route) => [
      'link',
      {
        rel: 'canonical',
        href: `${siteOrigin}${docsBase.replace(/\/$/, '')}${deployedRoutePath(route.routePath)}`,
      },
    ],
  ],
  search: {
    mode: 'local',
    versioned: true,
  },
  builderConfig: {
    server: {
      publicDir: {
        name: path.join(__dirname, 'docs', 'public'),
      },
    },
  },
  themeConfig: {
    darkMode: 'force-light',
    enableContentAnimation: true,
    lastUpdated: true,
    localeRedirect: 'never',
    locales: registry.languages.map(({ id, label }) => ({
      lang: id,
      label,
      title: copy[id].title,
      description: copy[id].description,
      nav: navFor(id),
      sidebar: sidebarFor(id),
    })),
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/A3S-Lab/Cloud',
      },
    ],
  },
});
