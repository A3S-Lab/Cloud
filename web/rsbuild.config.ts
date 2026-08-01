import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

export default defineConfig({
  plugins: [pluginReact()],
  html: {
    title: 'A3S Workflow — AI 原生工作流引擎',
    meta: {
      description:
        '设计持久化 AI 工作流，所有节点均通过 A3S Runtime 独立执行。',
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
