import type { DockerfileBuildRecipe } from './source';

export const BUILD_PLAN_PROPOSAL_SCHEMA = 'a3s.cloud.build-plan-proposal.v1' as const;
export const BUILD_PLAN_CONTRACT_SCHEMA = 'a3s.cloud.build-plan.v1' as const;
export const BUILD_PLAN_DETECTOR_REVISION = 'p0.1-c1' as const;
export const MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES = 64 * 1024;
export const DEFAULT_BUILD_PLAN_LIST_LIMIT = 50;
export const MAX_BUILD_PLAN_LIST_LIMIT = 200;
export const WORKLOAD_PROFILE_CONTRACT_SCHEMA = 'a3s.cloud.workload-profile.v1' as const;
export const MAX_WORKLOAD_PROFILE_ACL_BYTES = 128 * 1024;
export const DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT = 50;
export const MAX_WORKLOAD_PROFILE_REVISION_LIST_LIMIT = 100;
export const PULL_REQUEST_PREVIEW_POLICY_CONTRACT_SCHEMA =
  'a3s.cloud.pull-request-preview-policy.v1' as const;
export const MAX_PULL_REQUEST_PREVIEW_POLICY_ACL_BYTES = 16 * 1024;
export const DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT = 50;
export const MAX_PREVIEW_POLICY_REVISION_LIST_LIMIT = 100;
export const MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER = Number.MAX_SAFE_INTEGER;

export type BuildPlanDetectorKind = 'asset_acl' | 'dockerfile';
export type BuildPlanDetectionDiagnosticCode =
  | 'asset_build_recipe_missing'
  | 'empty_dockerfile'
  | 'no_supported_layout';

export interface BuildPlanSource {
  sourceIdentityDigest: string;
  commitSha: string;
  sourceContentDigest: string;
}

export interface BuildPlanProposal {
  schema: typeof BUILD_PLAN_PROPOSAL_SCHEMA;
  proposalAcl: string;
  proposalDigest: string;
  detector: BuildPlanDetectorKind;
  detectorRevision: typeof BUILD_PLAN_DETECTOR_REVISION;
  projectRoot: string;
  evidencePath: string;
  evidenceDigest: string;
  recipe: DockerfileBuildRecipe;
}

export interface BuildPlanDetectionDiagnostic {
  code: BuildPlanDetectionDiagnosticCode;
  path: string | null;
}

export interface BuildPlanDetection {
  source: BuildPlanSource;
  proposals: BuildPlanProposal[];
  diagnostics: BuildPlanDetectionDiagnostic[];
}

export interface AcceptedBuildPlan {
  organizationId: string;
  projectId: string;
  environmentId: string;
  buildPlanId: string;
  sourceRevisionId: string;
  contractSchema: typeof BUILD_PLAN_CONTRACT_SCHEMA;
  contractAcl: string;
  contractDigest: string;
  proposal: BuildPlanProposal;
  aggregateVersion: 1;
  acceptedBy: string;
  acceptedAt: string;
}

export interface DetectBuildPlansInput {
  sourceRevisionId: string;
}

export interface AcceptBuildPlanInput {
  sourceRevisionId: string;
  proposalAcl: string;
}

export interface BuildPlanMutationResult {
  buildPlan: AcceptedBuildPlan;
  replayed: boolean;
}

export type WorkloadProfileKind = 'web' | 'worker' | 'scheduled_task';
export type ScheduledTaskCatchUpPolicy = 'skip' | 'latest';

export interface WorkloadProfileProcess {
  command: string[];
  args: string[];
  workingDirectory: string | null;
  environment: Record<string, string>;
}

export type WorkloadProfileSecretTarget =
  | { kind: 'environment'; variable: string }
  | { kind: 'file'; path: string; mode: number }
  | { kind: 'registry_credential' };

export interface WorkloadProfileSecretBinding {
  name: string;
  secretId: string;
  version: number;
  target: WorkloadProfileSecretTarget;
}

export interface WorkloadProfileResources {
  cpuMillis: number;
  memoryBytes: number;
  pids: number;
  ephemeralStorageBytes: number | null;
  executionTimeoutMs: number | null;
}

export interface WorkloadProfileServicePort {
  name: string;
  containerPort: number;
}

export interface WorkloadProfileHttpHealthCheck {
  portName: string;
  path: string;
  intervalMs: number;
  timeoutMs: number;
  healthyThreshold: number;
  unhealthyThreshold: number;
  stabilizationWindowMs: number;
}

export interface ScheduledTaskRetryPolicy {
  maximumAttempts: number;
  initialBackoffMs: number;
  maximumBackoffMs: number;
}

export interface ScheduledTaskHistoryPolicy {
  successfulLimit: number;
  failedLimit: number;
  maximumAgeDays: number;
}

export interface ScheduledTaskSchedule {
  expression: string;
  timezone: string;
  catchUp: ScheduledTaskCatchUpPolicy;
  maximumConcurrency: number;
  misfireGraceMs: number;
  retry: ScheduledTaskRetryPolicy;
  history: ScheduledTaskHistoryPolicy;
}

export interface WorkloadProfileSpec {
  name: string;
  kind: WorkloadProfileKind;
  process: WorkloadProfileProcess;
  secrets: WorkloadProfileSecretBinding[];
  resources: WorkloadProfileResources;
  ports: WorkloadProfileServicePort[];
  health: WorkloadProfileHttpHealthCheck | null;
  publicPort: string | null;
  schedule: ScheduledTaskSchedule | null;
}

export interface AcceptedWorkloadProfileRevision {
  organizationId: string;
  projectId: string;
  environmentId: string;
  workloadProfileId: string;
  workloadProfileRevisionId: string;
  revisionNumber: number;
  buildPlanId: string;
  sourceRevisionId: string;
  contractSchema: typeof WORKLOAD_PROFILE_CONTRACT_SCHEMA;
  contractAcl: string;
  contractDigest: string;
  buildPlanDigest: string;
  projectRoot: string;
  profile: WorkloadProfileSpec;
  acceptedBy: string;
  acceptedAt: string;
}

export interface AcceptWorkloadProfileInput {
  buildPlanId: string;
  profileAcl: string;
}

export interface WorkloadProfileMutationResult {
  workloadProfileRevision: AcceptedWorkloadProfileRevision;
  replayed: boolean;
}

export type PreviewForkPolicy = 'deny' | 'isolated';
export type PullRequestPreviewChangeKind = 'opened' | 'synchronized' | 'reopened' | 'closed';
export type PullRequestPreviewStatus = 'active' | 'cleanup_required';
export type PullRequestPreviewCleanupReason =
  | 'pull_request_closed'
  | 'pull_request_merged'
  | 'fork_denied'
  | 'expired';

export interface PreviewGitRepository {
  provider: 'github';
  canonicalUrl: string;
}

export interface PreviewQuota {
  maximumWorkloads: number;
  cpuMillis: number;
  memoryBytes: number;
  ephemeralStorageBytes: number;
}

export interface PullRequestPreviewPolicy {
  ownerPrincipalId: string;
  installationId: number;
  baseRepository: PreviewGitRepository;
  baseBranch: string;
  lifetimeSeconds: number;
  maximumActivePreviews: number;
  forkPolicy: PreviewForkPolicy;
  allowProtectedSecretsForTrustedSources: boolean;
  quota: PreviewQuota;
}

export interface AcceptedPullRequestPreviewPolicyRevision {
  organizationId: string;
  projectId: string;
  sourceEnvironmentId: string;
  sourceSubscriptionId: string;
  pullRequestPreviewPolicyRevisionId: string;
  revisionNumber: number;
  contractSchema: typeof PULL_REQUEST_PREVIEW_POLICY_CONTRACT_SCHEMA;
  contractAcl: string;
  contractDigest: string;
  policy: PullRequestPreviewPolicy;
  acceptedBy: string;
  acceptedAt: string;
}

export interface AcceptPullRequestPreviewPolicyInput {
  sourceSubscriptionId: string;
  policyAcl: string;
}

export interface PullRequestPreviewPolicyMutationResult {
  previewPolicyRevision: AcceptedPullRequestPreviewPolicyRevision;
  replayed: boolean;
}

export interface PullRequestPreview {
  organizationId: string;
  projectId: string;
  sourceEnvironmentId: string;
  sourceSubscriptionId: string;
  previewId: string;
  environmentId: string;
  environmentName: string;
  pullRequestId: number;
  pullRequestNumber: number;
  policyRevisionId: string;
  policyRevisionNumber: number;
  policyAcceptedAt: string;
  policy: PullRequestPreviewPolicy;
  headRepository: PreviewGitRepository | null;
  headBranch: string;
  headCommitSha: string;
  providerCreatedAt: string;
  lastProviderUpdatedAt: string;
  lastChangeKind: PullRequestPreviewChangeKind;
  lastMerged: boolean;
  expiresAt: string;
  status: PullRequestPreviewStatus;
  cleanupReason: PullRequestPreviewCleanupReason | null;
  cleanupRequestedAt: string | null;
  aggregateVersion: number;
  isFork: boolean;
  protectedSecretsEligible: boolean;
}

export function validateBuildPlanProposalAcl(acl: string): void {
  validateCanonicalAcl(acl, MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES, 'BuildPlan proposal ACL');
}

export function validateBuildPlanListLimit(limit: number): void {
  validateListLimit(limit, MAX_BUILD_PLAN_LIST_LIMIT, 'BuildPlan list limit');
}

export function validateWorkloadProfileAcl(acl: string): void {
  validateCanonicalAcl(acl, MAX_WORKLOAD_PROFILE_ACL_BYTES, 'WorkloadProfile ACL');
}

export function validateWorkloadProfileRevisionListLimit(limit: number): void {
  validateListLimit(limit, MAX_WORKLOAD_PROFILE_REVISION_LIST_LIMIT, 'WorkloadProfile revision list limit');
}

export function validatePullRequestPreviewPolicyAcl(acl: string): void {
  validateCanonicalAcl(acl, MAX_PULL_REQUEST_PREVIEW_POLICY_ACL_BYTES, 'Pull-request Preview Policy ACL');
}

export function validatePreviewPolicyRevisionListLimit(limit: number): void {
  validateListLimit(
    limit,
    MAX_PREVIEW_POLICY_REVISION_LIST_LIMIT,
    'Pull-request Preview Policy revision list limit'
  );
}

export function validatePullRequestPreviewId(pullRequestId: number): void {
  validatePortablePositiveInteger(pullRequestId, 'Pull-request identity');
}

function validateCanonicalAcl(acl: string, maximumBytes: number, label: string): void {
  const byteLength = new TextEncoder().encode(acl).byteLength;
  if (byteLength < 1 || byteLength > maximumBytes || acl.replaceAll('\r\n', '').includes('\r')) {
    throw new RangeError(
      `${label} must contain between 1 and ${maximumBytes} UTF-8 bytes without bare carriage returns`
    );
  }
}

function validateListLimit(limit: number, maximum: number, label: string): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > maximum) {
    throw new RangeError(`${label} must be between 1 and ${maximum}`);
  }
}

function validatePortablePositiveInteger(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value < 1 || value > MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER) {
    throw new RangeError(`${label} must be a portable positive integer`);
  }
}
