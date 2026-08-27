import type { AcceptedBuildPlan, BuildPlanDetection, BuildPlanMutationResult } from '@a3s/cloud-client';
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
