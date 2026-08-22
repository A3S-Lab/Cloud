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

export type RecipientContactStatus = 'pending' | 'verified' | 'revoked';

export interface RecipientContact {
  id: string;
  principalId: string;
  addressDigest: string;
  addressHint: string;
  aggregateVersion: number;
  status: RecipientContactStatus;
  createdAt: string;
  updatedAt: string;
  verifiedAt: string | null;
  revokedAt: string | null;
}

export interface RecipientContactMutationResult extends RecipientContact {
  replayed: boolean;
}

export interface RequestRecipientContactVerificationInput {
  address: string;
}

export interface CompleteRecipientContactVerificationInput {
  proof: string;
}

export const MAX_RECIPIENT_CONTACT_ADDRESS_BYTES = 254;
export const MAX_RECIPIENT_CONTACT_PROOF_BYTES = 4096;

export function validateRecipientContactAddress(value: string): void {
  const invalid = () => new TypeError('recipient contact address must be a bounded canonical ASCII mailbox');
  if (
    typeof value !== 'string' ||
    value.length < 3 ||
    value.length > MAX_RECIPIENT_CONTACT_ADDRESS_BYTES ||
    !/^[\x21-\x7e]+$/.test(value)
  ) {
    throw invalid();
  }
  const parts = value.split('@');
  if (parts.length !== 2) {
    throw invalid();
  }
  const [local = '', domain = ''] = parts;
  if (
    local.length < 1 ||
    local.length > 64 ||
    local.startsWith('.') ||
    local.endsWith('.') ||
    local.includes('..') ||
    !/^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+$/.test(local) ||
    domain.length < 1 ||
    domain.length > 253 ||
    domain.startsWith('.') ||
    domain.endsWith('.') ||
    domain.includes('..')
  ) {
    throw invalid();
  }
  for (const label of domain.split('.')) {
    if (label.length < 1 || label.length > 63 || !/^[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?$/.test(label)) {
      throw invalid();
    }
  }
}

export function validateRecipientContactProof(value: string): void {
  if (
    typeof value !== 'string' ||
    value.length > MAX_RECIPIENT_CONTACT_PROOF_BYTES ||
    !/^a3srcv1\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/.test(value)
  ) {
    throw new TypeError('recipient contact proof is invalid');
  }
}

export function validateExpectedRecipientContactVersion(value: number): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new TypeError('expected recipient contact version must be a positive safe integer');
  }
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
