export type SecretState = 'active' | 'revoked';
export type SecretVersionState = 'active' | 'revoked';

export interface Secret {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  name: string;
  state: SecretState;
  currentVersion: number;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  revokedAt: string | null;
}

export interface SecretVersion {
  version: number;
  state: SecretVersionState;
  aggregateVersion: number;
  createdAt: string;
  revokedAt: string | null;
}

export interface SecretDetails extends Secret {
  versions: SecretVersion[];
}

export interface SecretMutationResult extends Secret {
  version: SecretVersion;
  replayed: boolean;
}
