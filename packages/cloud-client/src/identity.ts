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

export interface OidcAuthorizationStart {
  authorizationUrl: string;
}

export interface OidcLinkResult {
  kind: 'linked';
  linkId: string;
  providerKey: string;
  principalId: string;
  aggregateVersion: number;
  createdAt: string;
  lastVerifiedAt: string;
}

export interface OidcLoginResult {
  kind: 'login';
  token: ApiToken;
  credential: string;
}

export type OidcCallbackResult = OidcLinkResult | OidcLoginResult;

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

export interface CreateMembershipInput {
  principalKind: IdentityPrincipalKind;
  name: string;
  role: MembershipRole;
}

export type MembershipInvitationStatus = 'pending' | 'accepted' | 'revoked' | 'expired';

export interface MembershipInvitation {
  id: string;
  organizationId: string;
  principalId: string;
  role: MembershipRole;
  invitedByPrincipalId: string;
  status: MembershipInvitationStatus;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  expiresAt: string;
  acceptedMembershipId: string | null;
  acceptedAt: string | null;
  revokedAt: string | null;
}

export interface MembershipInvitationMutationResult extends MembershipInvitation {
  replayed: boolean;
}

export interface MembershipInvitationAcceptanceResult {
  invitation: MembershipInvitation;
  membership: Membership;
  replayed: boolean;
}

export interface CreateMembershipInvitationInput {
  principalId: string;
  role: MembershipRole;
  expiresAt: string;
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
