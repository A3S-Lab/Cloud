import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

const apiOrigin = process.env.A3S_CLOUD_API_ORIGIN ?? 'http://127.0.0.1:8080';

export default defineConfig({
  plugins: [pluginReact()],
  source: {
    entry: {
      index: './src/main.tsx',
    },
  },
  html: {
    title: 'A3S Cloud',
    meta: {
      description: 'Operate applications and A3S assets on infrastructure you own',
      'theme-color': '#101713',
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
