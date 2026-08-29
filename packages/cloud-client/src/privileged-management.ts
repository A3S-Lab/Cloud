import { validateNonNilUuid } from './validation';

export const PLATFORM_ROLE_POLICY_MAX_ACL_BYTES = 64 * 1024;
export const TENANT_SUPPORT_GRANT_MAX_ACL_BYTES = 64 * 1024;
export const TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES = 32 * 1024;
export const WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES = 64 * 1024;
export const DEFAULT_WORKLOAD_TRUST_REVISION_LIST_LIMIT = 50;
export const MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT = 100;

export type PlatformRole = 'platform_owner' | 'platform_admin' | 'platform_operator' | 'security_auditor';

export type PlatformPermission =
  | 'platform:read'
  | 'platform:role-policy:read'
  | 'platform:role-policy:manage'
  | 'platform:role-binding:read'
  | 'platform:role-binding:manage'
  | 'platform:identity-root:read'
  | 'platform:identity-root:manage'
  | 'platform:workload-trust:read'
  | 'platform:workload-trust:manage'
  | 'platform:node-pool:read'
  | 'platform:node-pool:manage'
  | 'platform:capacity:read'
  | 'platform:capacity:manage'
  | 'platform:provider:read'
  | 'platform:provider:manage'
  | 'platform:provider-credential:manage'
  | 'platform:registry-trust:read'
  | 'platform:registry-trust:manage'
  | 'platform:configuration:read'
  | 'platform:configuration:manage'
  | 'platform:upgrade:read'
  | 'platform:upgrade:manage'
  | 'platform:backup-restore:read'
  | 'platform:backup-restore:manage'
  | 'platform:tenant-lifecycle:read'
  | 'platform:tenant-lifecycle:manage'
  | 'platform:tenant-support:read'
  | 'platform:tenant-support:manage'
  | 'platform:tenant-support:use'
  | 'platform:operations:read'
  | 'platform:operations:execute'
  | 'platform:audit:read'
  | 'platform:audit:export'
  | 'platform:audit-retention:manage'
  | 'platform:security-findings:read'
  | 'platform:recovery:execute';

export interface PlatformRolePermission {
  role: PlatformRole;
  permissions: PlatformPermission[];
}

export interface PlatformRolePolicy {
  id: string;
  installationId: string;
  policyId: string;
  revisionNumber: number;
  canonicalAcl: string;
  digest: string;
  rolePermissions: PlatformRolePermission[];
  acceptedBy: string;
  acceptedAt: string;
}

export interface PlatformRolePolicyMutationResult extends PlatformRolePolicy {
  replayed: boolean;
}

export interface PlatformRoleBinding {
  id: string;
  installationId: string;
  principalId: string;
  role: PlatformRole;
  aggregateVersion: number;
  createdBy: string;
  updatedBy: string;
  createdAt: string;
  updatedAt: string;
  revokedAt: string | null;
}

export interface PlatformRoleBindingMutationResult extends PlatformRoleBinding {
  replayed: boolean;
}

export interface AcceptPlatformRolePolicyInput {
  canonicalAcl: string;
  revisionNumber: number;
  expectedCurrentRevisionId: string;
}

export interface TrustDomainRevision {
  installationId: string;
  trustDomainId: string;
  revisionId: string;
  revisionNumber: number;
  name: string;
  canonicalAcl: string;
  digest: string;
  acceptedBy: string;
  acceptedAt: string;
}

export interface TrustDomainRevisionMutationResult extends TrustDomainRevision {
  replayed: boolean;
}

export interface WorkloadIdentityPolicyRevision {
  installationId: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  policyId: string;
  revisionId: string;
  revisionNumber: number;
  trustDomainId: string;
  trustDomainRevisionId: string;
  workloadId: string;
  workloadRevisionId: string;
  nodePoolId: string;
  canonicalAcl: string;
  digest: string;
  acceptedBy: string;
  acceptedAt: string;
}

export interface WorkloadIdentityPolicyRevisionMutationResult extends WorkloadIdentityPolicyRevision {
  replayed: boolean;
}

export interface AcceptTrustDomainRevisionInput {
  canonicalAcl: string;
  revisionNumber: number;
  expectedPreviousRevisionId?: string | null;
}

export interface AcceptWorkloadIdentityPolicyRevisionInput {
  canonicalAcl: string;
  revisionNumber: number;
  expectedPreviousRevisionId?: string | null;
}

export interface WorkloadTrustRevisionListOptions {
  limit?: number;
}

export interface CreatePlatformRoleBindingInput {
  principalId: string;
  role: PlatformRole;
  expectedPolicyRevisionId: string;
}

export interface ChangePlatformRoleBindingInput {
  role: PlatformRole;
  expectedVersion: number;
  expectedPolicyRevisionId: string;
}

export type TenantSupportPermission =
  | 'tenant-support:resource-metadata:read'
  | 'tenant-support:health:read'
  | 'tenant-support:audit:read'
  | 'tenant-support:deployment:read'
  | 'tenant-support:deployment:recover'
  | 'tenant-support:route:recover'
  | 'tenant-support:runtime:restart';

interface TenantSupportScopeBase {
  installationId: string;
  organizationId: string;
}

export type TenantSupportScope =
  | (TenantSupportScopeBase & {
      kind: 'organization';
      projectId: null;
      environmentId: null;
    })
  | (TenantSupportScopeBase & {
      kind: 'project';
      projectId: string;
      environmentId: null;
    })
  | (TenantSupportScopeBase & {
      kind: 'environment';
      projectId: string;
      environmentId: string;
    });

export interface DecisionEvidence {
  id: string;
  digest: string;
}

export interface TenantSupportGrantProposal {
  id: string;
  principalId: string;
  scope: TenantSupportScope;
  permissions: TenantSupportPermission[];
  caseReference: string;
  justificationDigest: string;
  mode: 'standard' | 'break_glass';
  approvalRequirement: 'single' | 'dual';
  approverIds: string[];
  tenantNotification: 'required' | 'policy_exempt';
  securityAlertRequired: boolean;
  postIncidentReviewRequired: boolean;
  startsAt: string;
  expiresAt: string;
  canonicalAcl: string;
  contractDigest: string;
  requestedBy: string;
  authentication: DecisionEvidence;
  requestedAt: string;
}

export interface TenantSupportGrantApproval {
  grantId: string;
  contractDigest: string;
  approverId: string;
  authentication: DecisionEvidence;
  policyRevisionId: string;
  policyDigest: string;
  bindingId: string;
  bindingVersion: number;
  approvedAt: string;
  digest: string;
}

export interface TenantSupportGrantLifecycle {
  id: string;
  aggregateVersion: number;
  revocationGeneration: number;
  acceptedAt: string;
  revokedAt: string | null;
  revokedBy: string | null;
}

export interface TenantSupportGrant {
  proposal: TenantSupportGrantProposal;
  approvals: TenantSupportGrantApproval[];
  grant: TenantSupportGrantLifecycle | null;
}

export interface TenantSupportGrantProposalMutationResult {
  proposal: TenantSupportGrantProposal;
  replayed: boolean;
}

export interface TenantSupportGrantApprovalOutcome {
  proposal: TenantSupportGrantProposal;
  approval: TenantSupportGrantApproval;
  grant: TenantSupportGrantLifecycle | null;
}

export interface TenantSupportGrantApprovalMutationResult {
  outcome: TenantSupportGrantApprovalOutcome;
  replayed: boolean;
}

export interface TenantSupportGrantMutationResult {
  grant: TenantSupportGrantLifecycle;
  replayed: boolean;
}

export interface ProposeTenantSupportGrantInput {
  canonicalAcl: string;
}

export interface ApproveTenantSupportGrantInput {
  expectedContractDigest: string;
}

const PLATFORM_ROLES: ReadonlySet<PlatformRole> = new Set([
  'platform_owner',
  'platform_admin',
  'platform_operator',
  'security_auditor',
]);

export function validateAcceptPlatformRolePolicyInput(input: AcceptPlatformRolePolicyInput): void {
  validateCanonicalAcl(input?.canonicalAcl, PLATFORM_ROLE_POLICY_MAX_ACL_BYTES, 'platform role policy ACL');
  validatePositiveSafeInteger(input?.revisionNumber, 'platform role policy revision number');
  validateNonNilUuid(input?.expectedCurrentRevisionId, 'expected current platform role policy revision ID');
}

export function validateAcceptTrustDomainRevisionInput(input: AcceptTrustDomainRevisionInput): void {
  validateCanonicalAcl(input?.canonicalAcl, TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES, 'trust-domain ACL');
  validateImmutableRevisionControl(input?.revisionNumber, input?.expectedPreviousRevisionId, 'trust-domain');
}

export function validateAcceptWorkloadIdentityPolicyRevisionInput(
  input: AcceptWorkloadIdentityPolicyRevisionInput
): void {
  validateCanonicalAcl(
    input?.canonicalAcl,
    WORKLOAD_IDENTITY_POLICY_MAX_ACL_BYTES,
    'workload identity policy ACL'
  );
  validateImmutableRevisionControl(
    input?.revisionNumber,
    input?.expectedPreviousRevisionId,
    'workload identity policy'
  );
}

export function encodeWorkloadTrustRevisionListOptions(
  options: WorkloadTrustRevisionListOptions = {}
): string {
  const limit = options.limit ?? DEFAULT_WORKLOAD_TRUST_REVISION_LIST_LIMIT;
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT) {
    throw new RangeError(
      `workload trust revision list limit must be between 1 and ${MAX_WORKLOAD_TRUST_REVISION_LIST_LIMIT}`
    );
  }
  return `?limit=${limit}`;
}

export function validateCreatePlatformRoleBindingInput(input: CreatePlatformRoleBindingInput): void {
  validateNonNilUuid(input?.principalId, 'platform role binding Principal ID');
  validatePlatformRole(input?.role);
  validateNonNilUuid(input?.expectedPolicyRevisionId, 'expected platform role policy revision ID');
}

export function validateChangePlatformRoleBindingInput(input: ChangePlatformRoleBindingInput): void {
  validatePlatformRole(input?.role);
  validatePrivilegedExpectedVersion(input?.expectedVersion);
  validateNonNilUuid(input?.expectedPolicyRevisionId, 'expected platform role policy revision ID');
}

export function validateProposeTenantSupportGrantInput(input: ProposeTenantSupportGrantInput): void {
  validateCanonicalAcl(input?.canonicalAcl, TENANT_SUPPORT_GRANT_MAX_ACL_BYTES, 'tenant-support grant ACL');
}

export function validateApproveTenantSupportGrantInput(input: ApproveTenantSupportGrantInput): void {
  validateTenantSupportContractDigest(input?.expectedContractDigest);
}

export function validateTenantSupportContractDigest(value: unknown): asserts value is string {
  if (typeof value !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(value)) {
    throw new TypeError('expected tenant-support contract digest must be a canonical SHA-256 digest');
  }
}

export function validatePrivilegedExpectedVersion(value: number): void {
  validatePositiveSafeInteger(value, 'expected privileged aggregate version');
}

export function validatePlatformRole(value: unknown): asserts value is PlatformRole {
  if (typeof value !== 'string' || !PLATFORM_ROLES.has(value as PlatformRole)) {
    throw new TypeError('platform role must be one of the closed platform roles');
  }
}

function validateCanonicalAcl(value: string, maximumBytes: number, label: string): void {
  const byteLength = typeof value === 'string' ? new TextEncoder().encode(value).byteLength : 0;
  if (byteLength < 1 || byteLength > maximumBytes || value.replaceAll('\r\n', '').includes('\r')) {
    throw new RangeError(
      `${label} must contain between 1 and ${maximumBytes} UTF-8 bytes without bare carriage returns`
    );
  }
}

function validatePositiveSafeInteger(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError(`${label} must be a positive safe integer`);
  }
}

function validateImmutableRevisionControl(
  revisionNumber: number,
  expectedPreviousRevisionId: string | null | undefined,
  label: string
): void {
  validatePositiveSafeInteger(revisionNumber, `${label} revision number`);
  if (revisionNumber === 1) {
    if (expectedPreviousRevisionId !== undefined && expectedPreviousRevisionId !== null) {
      throw new RangeError(`${label} revision 1 must not declare an expected previous revision ID`);
    }
    return;
  }
  if (expectedPreviousRevisionId === undefined || expectedPreviousRevisionId === null) {
    throw new TypeError(`expected previous ${label} revision ID must be a non-nil UUID`);
  }
  validateNonNilUuid(expectedPreviousRevisionId, `expected previous ${label} revision ID`);
}
