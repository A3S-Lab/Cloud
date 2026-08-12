export interface ApiToken {
  id: string;
  organizationId: string;
  principalId: string;
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
  principalId?: string;
  expiresAt?: string | null;
}

export type MembershipRole = 'owner' | 'admin' | 'member' | 'restricted';
export type IdentityPrincipalKind = 'human' | 'service';

export interface Membership {
  id: string;
  organizationId: string;
  principalId: string;
  principalKind: IdentityPrincipalKind;
  principalName: string;
  principalAggregateVersion: number;
  principalDisabledAt: string | null;
  role: MembershipRole;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  revokedAt: string | null;
}

export interface MembershipMutationResult extends Membership {
  replayed: boolean;
}

export interface CreateServiceMembershipInput {
  name: string;
  role: MembershipRole;
}

export type ResourceGrantScope =
  | { kind: 'project'; projectId: string }
  | { kind: 'environment'; projectId: string; environmentId: string }
  | { kind: 'node'; nodeId: string };

export interface ResourceGrant {
  id: string;
  organizationId: string;
  membershipId: string;
  scope: ResourceGrantScope;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  revokedAt: string | null;
}

export interface ResourceGrantMutationResult extends ResourceGrant {
  replayed: boolean;
}

export interface CreateResourceGrantInput {
  scope: ResourceGrantScope;
}
