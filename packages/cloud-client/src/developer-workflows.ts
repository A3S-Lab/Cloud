import type { DockerfileBuildRecipe } from './source';

export const BUILD_PLAN_PROPOSAL_SCHEMA = 'a3s.cloud.build-plan-proposal.v1' as const;
export const BUILD_PLAN_CONTRACT_SCHEMA = 'a3s.cloud.build-plan.v1' as const;
export const BUILD_PLAN_DETECTOR_REVISION = 'p0.1-c1' as const;
export const MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES = 64 * 1024;
export const DEFAULT_BUILD_PLAN_LIST_LIMIT = 50;
export const MAX_BUILD_PLAN_LIST_LIMIT = 200;

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

export function validateBuildPlanProposalAcl(acl: string): void {
  const byteLength = new TextEncoder().encode(acl).byteLength;
  if (
    byteLength < 1 ||
    byteLength > MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES ||
    acl.replaceAll('\r\n', '').includes('\r')
  ) {
    throw new RangeError(
      `BuildPlan proposal ACL must contain between 1 and ${MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES} UTF-8 bytes without bare carriage returns`
    );
  }
}

export function validateBuildPlanListLimit(limit: number): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_BUILD_PLAN_LIST_LIMIT) {
    throw new RangeError(`BuildPlan list limit must be between 1 and ${MAX_BUILD_PLAN_LIST_LIMIT}`);
  }
}
