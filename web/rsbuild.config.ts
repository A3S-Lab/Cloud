import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

const apiOrigin = process.env.A3S_CLOUD_API_ORIGIN ?? 'http://127.0.0.1:8080';
const directionContract = `<!--
  THESIS: A3S OS presents autonomous workflow orchestration, the heterogeneous Agent Factory, security operations, and A3S Web as one governed enterprise AI system.
  OWN-WORLD: Pure white canvas, cool blue hairlines, one electric-blue brand field, compact humanist type, and semantic status color.
  STORY: Understand the three products, see how A3S Web unifies their operation, then inspect the shared architecture.
  FIRST VIEWPORT: One-line navigation, enterprise AI promise, and the sole A3S authority path.
  FORM: Enterprise product portal and a separately addressed operations client, preserving the approved Finogeeks-informed visual world; seed 90c8e585.
  FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md
-->`;

const directionContractPlugin = {
  name: 'a3s-cloud-direction-contract',
  setup(api: { modifyHTML: (handler: (html: string) => string) => void }) {
    api.modifyHTML((html) => {
      const localizedHtml = html.replace(/<html(?:\s[^>]*)?>/, '<html lang="zh-CN">');
      if (localizedHtml.includes('90c8e585')) return localizedHtml;

      return localizedHtml.replace(/<body([^>]*)>/, (openingTag) => `${openingTag}\n${directionContract}`);
    });
  },
};

export default defineConfig({
  plugins: [pluginReact(), directionContractPlugin],
  source: {
    entry: {
      index: './src/main.tsx',
    },
  },
  html: {
    title: 'A3S OS',
    favicon: './src/assets/favicon.svg',
    meta: {
      description: 'A3S OS 企业级 AI 操作系统：自主工作流编排、异构智能体工厂、安全监控中台与 A3S Web',
      'theme-color': '#ffffff',
    },
  },
  output: {
    cleanDistPath: true,
    distPath: {
      root: 'dist',
    },
    assetPrefix: '/',
  },
  server: {
    port: Number(process.env.A3S_CLOUD_WEB_DEV_PORT ?? 3010),
    proxy: {
      '/api': {
        target: apiOrigin,
        changeOrigin: true,
      },
    },
  },
});
