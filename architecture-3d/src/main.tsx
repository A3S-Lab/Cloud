import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app';
import './styles.css';

const root = document.getElementById('root');
if (!root) throw new Error('A3S Cloud architecture application root was not found');

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>
);
