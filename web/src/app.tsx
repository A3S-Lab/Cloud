import { useState } from 'react';
import { CloudConsole } from './features/console/cloud-console';
import { SignIn } from './features/session/sign-in';
import type { Organization } from './types/api';

const TOKEN_KEY = 'a3s-cloud.api-token';

export function App() {
  const [token, setToken] = useState(() => sessionStorage.getItem(TOKEN_KEY) ?? '');
  const [organizations, setOrganizations] = useState<Organization[]>([]);

  if (!token) {
    return (
      <SignIn
        onAuthenticated={(authenticatedToken, visibleOrganizations) => {
          sessionStorage.setItem(TOKEN_KEY, authenticatedToken);
          setOrganizations(visibleOrganizations);
          setToken(authenticatedToken);
        }}
      />
    );
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
