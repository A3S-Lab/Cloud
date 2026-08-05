import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app';
import { LanguageProvider } from './lib/i18n';
import './styles.css';
import './styles/console-navigation.css';
import './styles/workloads.css';
import './styles/workload-operations.css';
import './styles/search.css';
import './styles/builds.css';
import './styles/agents.css';
import './styles/architecture.css';
import './styles/redesign-base.css';
import './styles/redesign-session.css';
import './styles/redesign-console.css';
import './styles/redesign-features.css';
import './styles/redesign-responsive.css';

const root = document.getElementById('root');

if (!root) {
  throw new Error('A3S Cloud root element is missing');
}

createRoot(root).render(
  <StrictMode>
    <LanguageProvider>
      <App />
    </LanguageProvider>
  </StrictMode>
);
