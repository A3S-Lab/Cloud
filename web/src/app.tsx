import { useState } from 'react';
import { CloudConsole } from './features/console/cloud-console';
import { ControlPlaneAccess } from './features/session/control-plane-access';
import { ProjectHome } from './features/project/project-home';
import { LanguageSwitcher } from './lib/i18n';
import type { Organization } from './types/api';

const TOKEN_KEY = 'a3s-cloud.api-token';

export function App() {
  const [token, setToken] = useState(() => sessionStorage.getItem(TOKEN_KEY) ?? '');
  const [organizations, setOrganizations] = useState<Organization[]>([]);

  const authenticate = (authenticatedToken: string, visibleOrganizations: Organization[]) => {
    sessionStorage.setItem(TOKEN_KEY, authenticatedToken);
    setOrganizations(visibleOrganizations);
    setToken(authenticatedToken);
  };

  if (!token) {
    if (window.location.hash === '#console') {
      return (
        <main className='standalone-access'>
          <header>
            <div className='brand-lockup'>
              <span className='brand-mark' data-brand-mark aria-hidden='true'>
                A3
              </span>
              <span data-brand-name>A3S OS · A3S Web</span>
            </div>
            <LanguageSwitcher />
          </header>
          <ControlPlaneAccess onAuthenticated={authenticate} />
        </main>
      );
    }

    return <ProjectHome />;
  }

  return (
    <CloudConsole
      token={token}
      initialOrganizations={organizations}
      onSignOut={() => {
        sessionStorage.removeItem(TOKEN_KEY);
        setOrganizations([]);
        setToken('');
      }}
    />
  );
}
