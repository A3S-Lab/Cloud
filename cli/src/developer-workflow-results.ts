import type {
  AcceptedBuildPlan,
  AcceptedWorkloadProfileRevision,
  BuildPlanDetection,
  BuildPlanMutationResult,
  WorkloadProfileMutationResult,
} from '@a3s/cloud-client';
import { renderTable, type TableColumn } from './output';
import type { CommandResult } from './results';

const ACCEPTED_BUILD_PLAN_COLUMNS: readonly TableColumn<AcceptedBuildPlan>[] = [
  { header: 'ID', value: (row) => row.buildPlanId },
  { header: 'SOURCE REVISION', value: (row) => row.sourceRevisionId },
  { header: 'ROOT', value: (row) => row.proposal.projectRoot },
  { header: 'DETECTOR', value: (row) => row.proposal.detector },
  { header: 'CONTRACT DIGEST', value: (row) => row.contractDigest },
  { header: 'ACCEPTED AT', value: (row) => row.acceptedAt },
];

const ACCEPTED_WORKLOAD_PROFILE_REVISION_COLUMNS: readonly TableColumn<AcceptedWorkloadProfileRevision>[] = [
  { header: 'PROFILE ID', value: (row) => row.workloadProfileId },
  { header: 'REVISION ID', value: (row) => row.workloadProfileRevisionId },
  { header: 'REVISION', value: (row) => row.revisionNumber },
  { header: 'NAME', value: (row) => row.profile.name },
  { header: 'KIND', value: (row) => row.profile.kind },
  { header: 'BUILD PLAN', value: (row) => row.buildPlanId },
  { header: 'ROOT', value: (row) => row.projectRoot },
  { header: 'CONTRACT DIGEST', value: (row) => row.contractDigest },
  { header: 'ACCEPTED AT', value: (row) => row.acceptedAt },
];

export function buildPlanDetectionResult(detection: BuildPlanDetection): CommandResult {
  return {
    json: detection,
    table: renderTable(
      [detection],
      [
        { header: 'COMMIT', value: (row) => row.source.commitSha },
        { header: 'SOURCE DIGEST', value: (row) => row.source.sourceContentDigest },
        { header: 'PROPOSALS', value: (row) => row.proposals.length },
        {
          header: 'DIAGNOSTICS',
          value: (row) => row.diagnostics.map((diagnostic) => diagnostic.code).join(','),
        },
      ]
    ),
  };
}

export function acceptedBuildPlansResult(plans: AcceptedBuildPlan[]): CommandResult {
  return {
    json: plans,
    table: renderTable(plans, ACCEPTED_BUILD_PLAN_COLUMNS),
  };
}

export function acceptedBuildPlanResult(plan: AcceptedBuildPlan): CommandResult {
  return {
    json: plan,
    table: renderTable([plan], ACCEPTED_BUILD_PLAN_COLUMNS),
  };
}

export function buildPlanMutationResult(result: BuildPlanMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [result],
      [
        { header: 'ID', value: (row) => row.buildPlan.buildPlanId },
        { header: 'SOURCE REVISION', value: (row) => row.buildPlan.sourceRevisionId },
        { header: 'ROOT', value: (row) => row.buildPlan.proposal.projectRoot },
        { header: 'CONTRACT DIGEST', value: (row) => row.buildPlan.contractDigest },
        { header: 'REPLAYED', value: (row) => row.replayed },
      ]
    ),
  };
}

export function acceptedWorkloadProfileRevisionsResult(
  revisions: AcceptedWorkloadProfileRevision[]
): CommandResult {
  return {
    json: revisions,
    table: renderTable(revisions, ACCEPTED_WORKLOAD_PROFILE_REVISION_COLUMNS),
  };
}

export function acceptedWorkloadProfileRevisionResult(
  revision: AcceptedWorkloadProfileRevision
): CommandResult {
  return {
    json: revision,
    table: renderTable([revision], ACCEPTED_WORKLOAD_PROFILE_REVISION_COLUMNS),
  };
}

export function workloadProfileMutationResult(result: WorkloadProfileMutationResult): CommandResult {
  return {
    json: result,
    table: renderTable(
      [result],
      [
        {
          header: 'PROFILE ID',
          value: (row) => row.workloadProfileRevision.workloadProfileId,
        },
        {
          header: 'REVISION ID',
          value: (row) => row.workloadProfileRevision.workloadProfileRevisionId,
        },
        { header: 'REVISION', value: (row) => row.workloadProfileRevision.revisionNumber },
        { header: 'NAME', value: (row) => row.workloadProfileRevision.profile.name },
        { header: 'KIND', value: (row) => row.workloadProfileRevision.profile.kind },
        { header: 'REPLAYED', value: (row) => row.replayed },
      ]
    ),
  };
}
