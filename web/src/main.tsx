import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app';
import './styles.css';
import './styles/workloads.css';
import './styles/workload-operations.css';
import './styles/builds.css';

const root = document.getElementById('root');

if (!root) {
  throw new Error('A3S Cloud root element is missing');
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>
);
