import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

export default defineConfig({
  plugins: [pluginReact()],
  html: {
    title: 'A3S Workflow — AI Native Workflow Engine',
    meta: {
      description:
        'Design durable AI workflows where every node executes through A3S Runtime.',
      themeColor: '#101118',
    },
  },
  server: {
    port: 3000,
    host: '127.0.0.1',
    proxy: {
      '/api': 'http://127.0.0.1:8080',
    },
  },
  output: {
    distPath: {
      root: 'dist',
    },
  },
});
