import type {
  PlatformRoleBinding,
  PlatformRoleBindingMutationResult,
  PlatformRolePolicy,
  PlatformRolePolicyMutationResult,
  TenantSupportGrant,
  TenantSupportGrantApprovalMutationResult,
  TenantSupportGrantMutationResult,
  TenantSupportGrantProposal,
  TenantSupportGrantProposalMutationResult,
  TenantSupportScope,
  TrustDomainRevision,
  TrustDomainRevisionMutationResult,
  WorkloadIdentityPolicyRevision,
  WorkloadIdentityPolicyRevisionMutationResult,
  WorkloadIdentityProviderInspection,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const PLATFORM_ROLE_POLICY_COLUMNS: readonly TableColumn<PlatformRolePolicy>[] = [
  { header: 'REVISION ID', value: (row) => row.id },
  { header: 'REVISION', value: (row) => row.revisionNumber },
  { header: 'POLICY ID', value: (row) => row.policyId },
  { header: 'DIGEST', value: (row) => row.digest },
  { header: 'ROLES', value: (row) => row.rolePermissions.map(({ role }) => role).join(',') },
  { header: 'ACCEPTED BY', value: (row) => row.acceptedBy },
  { header: 'ACCEPTED AT', value: (row) => row.acceptedAt },
];

export function platformRolePolicyResult(row: PlatformRolePolicy): CommandResult {
  return { json: row, table: renderTable([row], PLATFORM_ROLE_POLICY_COLUMNS) };
}

export function platformRolePolicyMutationResult(row: PlatformRolePolicyMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...PLATFORM_ROLE_POLICY_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const TRUST_DOMAIN_REVISION_COLUMNS: readonly TableColumn<TrustDomainRevision>[] = [
  { header: 'TRUST DOMAIN', value: (row) => row.trustDomainId },
  { header: 'REVISION ID', value: (row) => row.revisionId },
  { header: 'REVISION', value: (row) => row.revisionNumber },
  { header: 'NAME', value: (row) => row.name },
  { header: 'DIGEST', value: (row) => row.digest },
  { header: 'ACCEPTED BY', value: (row) => row.acceptedBy },
  { header: 'ACCEPTED AT', value: (row) => row.acceptedAt },
];

export function trustDomainRevisionResult(row: TrustDomainRevision): CommandResult {
  return { json: row, table: renderTable([row], TRUST_DOMAIN_REVISION_COLUMNS) };
}

export function trustDomainRevisionListResult(rows: TrustDomainRevision[]): CommandResult {
  return { json: rows, table: renderTable(rows, TRUST_DOMAIN_REVISION_COLUMNS) };
}

export function trustDomainRevisionMutationResult(row: TrustDomainRevisionMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...TRUST_DOMAIN_REVISION_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const WORKLOAD_IDENTITY_PROVIDER_INSPECTION_COLUMNS: readonly TableColumn<WorkloadIdentityProviderInspection>[] =
  [
    { header: 'TRUST DOMAIN', value: (row) => row.trustDomainName },
    { header: 'REVISION ID', value: (row) => row.revision.revisionId },
    { header: 'PROVIDER PROFILE', value: (row) => row.providerProfileDigest },
    { header: 'TRUST BUNDLE', value: (row) => row.observedTrustBundleDigest },
    { header: 'OBSERVED FORMATS', value: (row) => row.observedIdentityFormats.join(',') },
    { header: 'DECLARED MAX TTL', value: (row) => row.declaredMaxCredentialLifetimeSeconds },
    {
      header: 'DECLARED REVOCATION EPOCHS',
      value: (row) => row.declaredSupportsRevocationEpochs,
    },
    { header: 'OBSERVED AT', value: (row) => row.observedAt },
  ];

export function workloadIdentityProviderInspectionResult(
  row: WorkloadIdentityProviderInspection
): CommandResult {
  return {
    json: row,
    table: renderTable([row], WORKLOAD_IDENTITY_PROVIDER_INSPECTION_COLUMNS),
  };
}

const WORKLOAD_IDENTITY_POLICY_REVISION_COLUMNS: readonly TableColumn<WorkloadIdentityPolicyRevision>[] = [
  { header: 'ORGANIZATION', value: (row) => row.organizationId },
  { header: 'POLICY', value: (row) => row.policyId },
  { header: 'REVISION ID', value: (row) => row.revisionId },
  { header: 'REVISION', value: (row) => row.revisionNumber },
  { header: 'WORKLOAD', value: (row) => row.workloadId },
  { header: 'NODE POOL', value: (row) => row.nodePoolId },
  { header: 'TRUST DOMAIN', value: (row) => row.trustDomainId },
  { header: 'DIGEST', value: (row) => row.digest },
  { header: 'ACCEPTED AT', value: (row) => row.acceptedAt },
];

export function workloadIdentityPolicyRevisionResult(row: WorkloadIdentityPolicyRevision): CommandResult {
  return { json: row, table: renderTable([row], WORKLOAD_IDENTITY_POLICY_REVISION_COLUMNS) };
}

export function workloadIdentityPolicyRevisionListResult(
  rows: WorkloadIdentityPolicyRevision[]
): CommandResult {
  return { json: rows, table: renderTable(rows, WORKLOAD_IDENTITY_POLICY_REVISION_COLUMNS) };
}

export function workloadIdentityPolicyRevisionMutationResult(
  row: WorkloadIdentityPolicyRevisionMutationResult
): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...WORKLOAD_IDENTITY_POLICY_REVISION_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

const PLATFORM_ROLE_BINDING_COLUMNS: readonly TableColumn<PlatformRoleBinding>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'ROLE', value: (row) => row.role },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'UPDATED BY', value: (row) => row.updatedBy },
  { header: 'UPDATED AT', value: (row) => row.updatedAt },
  { header: 'REVOKED AT', value: (row) => row.revokedAt ?? '' },
];

export function platformRoleBindingResult(row: PlatformRoleBinding): CommandResult {
  return { json: row, table: renderTable([row], PLATFORM_ROLE_BINDING_COLUMNS) };
}

export function platformRoleBindingMutationResult(row: PlatformRoleBindingMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [...PLATFORM_ROLE_BINDING_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

interface TenantSupportSummary {
  id: string;
  principalId: string;
  scope: string;
  mode: string;
  approvals: number;
  approvalRequirement: string;
  state: string;
  aggregateVersion: number | '';
  expiresAt: string;
}

const TENANT_SUPPORT_COLUMNS: readonly TableColumn<TenantSupportSummary>[] = [
  { header: 'ID', value: (row) => row.id },
  { header: 'PRINCIPAL', value: (row) => row.principalId },
  { header: 'SCOPE', value: (row) => row.scope },
  { header: 'MODE', value: (row) => row.mode },
  { header: 'APPROVALS', value: (row) => `${row.approvals}/${row.approvalRequirement}` },
  { header: 'STATE', value: (row) => row.state },
  { header: 'VERSION', value: (row) => row.aggregateVersion },
  { header: 'EXPIRES AT', value: (row) => row.expiresAt },
];

export function tenantSupportGrantResult(row: TenantSupportGrant): CommandResult {
  return {
    json: row,
    table: renderTable(
      [tenantSupportSummary(row.proposal, row.approvals.length, row.grant)],
      TENANT_SUPPORT_COLUMNS
    ),
  };
}

export function tenantSupportGrantProposalMutationResult(
  row: TenantSupportGrantProposalMutationResult
): CommandResult {
  const summary = { ...tenantSupportSummary(row.proposal, 0, null), replayed: row.replayed };
  return {
    json: row,
    table: renderTable(
      [summary],
      [...TENANT_SUPPORT_COLUMNS, { header: 'REPLAYED', value: (value) => value.replayed }]
    ),
  };
}

export function tenantSupportGrantApprovalMutationResult(
  row: TenantSupportGrantApprovalMutationResult
): CommandResult {
  const { outcome } = row;
  const summary = {
    ...tenantSupportSummary(
      outcome.proposal,
      outcome.grant === null ? 1 : approvalCount(outcome.proposal),
      outcome.grant
    ),
    approverId: outcome.approval.approverId,
    replayed: row.replayed,
  };
  return {
    json: row,
    table: renderTable(
      [summary],
      [
        ...TENANT_SUPPORT_COLUMNS,
        { header: 'APPROVER', value: (value) => value.approverId },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

export function tenantSupportGrantMutationResult(row: TenantSupportGrantMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.grant.id },
        { header: 'VERSION', value: (value) => value.grant.aggregateVersion },
        { header: 'REVOCATION', value: (value) => value.grant.revocationGeneration },
        { header: 'ACCEPTED AT', value: (value) => value.grant.acceptedAt },
        { header: 'REVOKED AT', value: (value) => value.grant.revokedAt ?? '' },
        { header: 'REVOKED BY', value: (value) => value.grant.revokedBy ?? '' },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

function tenantSupportSummary(
  proposal: TenantSupportGrantProposal,
  approvals: number,
  grant: TenantSupportGrant['grant']
): TenantSupportSummary {
  return {
    id: proposal.id,
    principalId: proposal.principalId,
    scope: tenantSupportScopeIdentity(proposal.scope),
    mode: proposal.mode,
    approvals,
    approvalRequirement: proposal.approvalRequirement,
    state: grant === null ? 'pending' : grant.revokedAt === null ? 'active' : 'revoked',
    aggregateVersion: grant?.aggregateVersion ?? '',
    expiresAt: proposal.expiresAt,
  };
}

function tenantSupportScopeIdentity(scope: TenantSupportScope): string {
  switch (scope.kind) {
    case 'organization':
      return `organization/${scope.organizationId}`;
    case 'project':
      return `project/${scope.projectId}`;
    case 'environment':
      return `environment/${scope.environmentId}`;
  }
}

function approvalCount(proposal: TenantSupportGrantProposal): number {
  return proposal.approvalRequirement === 'dual' ? 2 : 1;
}
