import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

const assetPrefix = process.env.A3S_ARCHITECTURE_BASE_PATH ?? '/';

export default defineConfig({
  plugins: [pluginReact()],
  source: {
    entry: {
      index: './src/main.tsx',
    },
    define: {
      'import.meta.env.A3S_ARCHITECTURE_BASE_PATH': JSON.stringify(assetPrefix),
    },
  },
  html: {
    template: './index.html',
    title: 'A3S Cloud · Interactive Architecture',
    favicon: './public/favicon.svg',
    meta: {
      'application-name': 'A3S Cloud Interactive Architecture',
      description:
        'Explore the A3S Cloud control plane, Runtime, Gateway, and product roadmap as an interactive 3D system map.',
      'theme-color': '#0b100d',
    },
  },
  output: {
    cleanDistPath: true,
    distPath: {
      root: 'dist',
    },
    assetPrefix,
  },
  server: {
    host: '127.0.0.1',
    port: Number(process.env.A3S_ARCHITECTURE_DEV_PORT ?? 4173),
  },
});
