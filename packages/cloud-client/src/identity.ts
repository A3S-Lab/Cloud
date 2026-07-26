export interface ApiToken {
  id: string;
  organizationId: string;
  name: string;
  scopes: string[];
  aggregateVersion: number;
  createdAt: string;
  expiresAt: string | null;
  revokedAt: string | null;
}

export interface ApiTokenMutationResult extends ApiToken {
  replayed: boolean;
}

export interface CreateApiTokenInput {
  name: string;
  token: string;
  scopes: string[];
  expiresAt?: string | null;
}
