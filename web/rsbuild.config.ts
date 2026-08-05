import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

const apiOrigin = process.env.A3S_CLOUD_API_ORIGIN ?? 'http://127.0.0.1:8080';
const directionContract = `<!--
  THESIS: A3S Cloud makes control authority and convergence readable in one scan; it refuses the generic dark DevOps dashboard.
  OWN-WORLD: Pure white canvas, cool blue hairlines, one electric-blue brand field, compact humanist type, and semantic status color.
  STORY: Select tenant context, understand current state, enter the owning workspace, act, and inspect durable evidence.
  FIRST VIEWPORT: One-line navigation, context controls, a concise workspace heading, a blue convergence field, operational truth, and the A3S authority chain.
  FORM: Operations-first enterprise control plane, approved composition B with Architecture from C and Sign In from A; seed 90c8e585.
  FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md
-->`;

const directionContractPlugin = {
  name: 'a3s-cloud-direction-contract',
  setup(api: { modifyHTML: (handler: (html: string) => string) => void }) {
    api.modifyHTML((html) => {
      const localizedHtml = html.replace(/<html(?:\s[^>]*)?>/, '<html lang="zh-CN">');
      if (localizedHtml.includes('90c8e585')) return localizedHtml;

      return localizedHtml.replace(
        /<body([^>]*)>/,
        (openingTag) => `${openingTag}\n${directionContract}`
      );
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
    title: 'A3S Cloud',
    favicon: './src/assets/favicon.svg',
    meta: {
      description: '在自有基础设施上运行应用、Agent 与 A3S 资产',
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
