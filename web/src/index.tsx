import React from 'react';
import { createRoot } from 'react-dom/client';
import { ReactFlowProvider } from '@xyflow/react';
import '@xyflow/react/dist/style.css';
import './styles.css';
import { App } from './App';

document.documentElement.lang = 'zh-CN';

const root = document.getElementById('root');
if (!root) {
  throw new Error('缺少 #root 应用挂载点');
}

createRoot(root).render(
  <React.StrictMode>
    <ReactFlowProvider>
      <App />
    </ReactFlowProvider>
  </React.StrictMode>,
);
