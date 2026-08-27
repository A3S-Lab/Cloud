import { describe, expect, it } from 'bun:test';
import {
  MAX_BUILD_PLAN_LIST_LIMIT,
  MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES,
  MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER,
  MAX_PREVIEW_POLICY_REVISION_LIST_LIMIT,
  MAX_PULL_REQUEST_PREVIEW_POLICY_ACL_BYTES,
  MAX_WORKLOAD_PROFILE_ACL_BYTES,
  MAX_WORKLOAD_PROFILE_REVISION_LIST_LIMIT,
  validateBuildPlanListLimit,
  validateBuildPlanProposalAcl,
  validatePreviewPolicyRevisionListLimit,
  validatePullRequestPreviewId,
  validatePullRequestPreviewPolicyAcl,
  validateWorkloadProfileAcl,
  validateWorkloadProfileRevisionListLimit,
} from './developer-workflows';

describe('Developer Workflows client boundary', () => {
  it('accepts only bounded UTF-8 BuildPlan proposal ACL text', () => {
    expect(() => validateBuildPlanProposalAcl('build_plan {}\n')).not.toThrow();
    expect(() => validateBuildPlanProposalAcl('build_plan {}\r\n')).not.toThrow();
    expect(() =>
      validateBuildPlanProposalAcl('é'.repeat(MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES / 2))
    ).not.toThrow();
    expect(() => validateBuildPlanProposalAcl('')).toThrow(RangeError);
    expect(() => validateBuildPlanProposalAcl('a'.repeat(MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES + 1))).toThrow(
      RangeError
    );
    expect(() => validateBuildPlanProposalAcl('é'.repeat(MAX_BUILD_PLAN_PROPOSAL_ACL_BYTES / 2 + 1))).toThrow(
      RangeError
    );
    expect(() => validateBuildPlanProposalAcl('build_plan {}\rbroken')).toThrow(RangeError);
  });

  it('keeps the accepted BuildPlan read bound aligned with the public contract', () => {
    expect(() => validateBuildPlanListLimit(1)).not.toThrow();
    expect(() => validateBuildPlanListLimit(MAX_BUILD_PLAN_LIST_LIMIT)).not.toThrow();
    for (const invalid of [0, MAX_BUILD_PLAN_LIST_LIMIT + 1, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
      expect(() => validateBuildPlanListLimit(invalid)).toThrow(RangeError);
    }
  });

  it('keeps WorkloadProfile ACL and revision bounds aligned without parsing ACL', () => {
    expect(() => validateWorkloadProfileAcl('workload_profile {}\n')).not.toThrow();
    expect(() => validateWorkloadProfileAcl('workload_profile {}\r\n')).not.toThrow();
    expect(() => validateWorkloadProfileAcl('')).toThrow(RangeError);
    expect(() => validateWorkloadProfileAcl('a'.repeat(MAX_WORKLOAD_PROFILE_ACL_BYTES + 1))).toThrow(
      RangeError
    );
    expect(() => validateWorkloadProfileAcl('workload_profile {}\rbroken')).toThrow(RangeError);
    expect(() => validateWorkloadProfileRevisionListLimit(1)).not.toThrow();
    expect(() =>
      validateWorkloadProfileRevisionListLimit(MAX_WORKLOAD_PROFILE_REVISION_LIST_LIMIT)
    ).not.toThrow();
    for (const invalid of [
      0,
      MAX_WORKLOAD_PROFILE_REVISION_LIST_LIMIT + 1,
      1.5,
      Number.MAX_SAFE_INTEGER + 1,
    ]) {
      expect(() => validateWorkloadProfileRevisionListLimit(invalid)).toThrow(RangeError);
    }
  });

  it('keeps Preview Policy ACL, history, and PR identities bounded without parsing ACL', () => {
    expect(() => validatePullRequestPreviewPolicyAcl('pull_request_preview_policy {}\n')).not.toThrow();
    expect(() => validatePullRequestPreviewPolicyAcl('pull_request_preview_policy {}\r\n')).not.toThrow();
    expect(() => validatePullRequestPreviewPolicyAcl('')).toThrow(RangeError);
    expect(() =>
      validatePullRequestPreviewPolicyAcl('a'.repeat(MAX_PULL_REQUEST_PREVIEW_POLICY_ACL_BYTES + 1))
    ).toThrow(RangeError);
    expect(() => validatePullRequestPreviewPolicyAcl('policy {}\rbroken')).toThrow(RangeError);

    for (const valid of [1, MAX_PREVIEW_POLICY_REVISION_LIST_LIMIT]) {
      expect(() => validatePreviewPolicyRevisionListLimit(valid)).not.toThrow();
    }
    for (const invalid of [0, MAX_PREVIEW_POLICY_REVISION_LIST_LIMIT + 1, 1.5]) {
      expect(() => validatePreviewPolicyRevisionListLimit(invalid)).toThrow(RangeError);
    }
    for (const valid of [1, MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER]) {
      expect(() => validatePullRequestPreviewId(valid)).not.toThrow();
    }
    for (const invalid of [0, -1, 1.5, MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER + 1]) {
      expect(() => validatePullRequestPreviewId(invalid)).toThrow(RangeError);
    }
  });
});
