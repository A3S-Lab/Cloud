import type {
  WorkflowDefinition,
  WorkflowDefinitionMutationResult,
  WorkflowGoal,
  WorkflowGoalMutationResult,
  WorkflowPlanRevision,
  WorkflowRevision,
  WorkflowRevisionSummary,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const WORKFLOW_DEFINITION_COLUMNS = [
  { header: 'ID', value: (row: WorkflowDefinition) => row.id },
  { header: 'NAME', value: (row: WorkflowDefinition) => row.name },
  { header: 'REVISION', value: (row: WorkflowDefinition) => row.currentRevisionNumber },
  { header: 'DIGEST', value: (row: WorkflowDefinition) => row.currentRevisionDigest },
  { header: 'VERSION', value: (row: WorkflowDefinition) => row.aggregateVersion },
  { header: 'UPDATED AT', value: (row: WorkflowDefinition) => row.updatedAt },
] as const;

export function workflowDefinitionsResult(rows: WorkflowDefinition[]): CommandResult {
  return { json: rows, table: renderTable(rows, WORKFLOW_DEFINITION_COLUMNS) };
}

export function workflowDefinitionResult(row: WorkflowDefinition): CommandResult {
  return { json: row, table: renderTable([row], WORKFLOW_DEFINITION_COLUMNS) };
}

export function workflowDefinitionMutationResult(row: WorkflowDefinitionMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.workflowDefinition.id },
        { header: 'NAME', value: (value) => value.workflowDefinition.name },
        { header: 'REVISION', value: (value) => value.revision.revisionNumber },
        { header: 'DIGEST', value: (value) => value.revision.contentDigest },
        { header: 'PAYLOADS', value: (value) => value.revision.payloadCount },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

const WORKFLOW_REVISION_COLUMNS = [
  { header: 'ID', value: (row: WorkflowRevisionSummary) => row.id },
  { header: 'NUMBER', value: (row: WorkflowRevisionSummary) => row.revisionNumber },
  { header: 'DIGEST', value: (row: WorkflowRevisionSummary) => row.contentDigest },
  { header: 'PAYLOADS', value: (row: WorkflowRevisionSummary) => row.payloadCount },
  { header: 'PARENT', value: (row: WorkflowRevisionSummary) => row.parentRevisionId },
  { header: 'CREATED AT', value: (row: WorkflowRevisionSummary) => row.createdAt },
] as const;

export function workflowRevisionsResult(rows: WorkflowRevisionSummary[]): CommandResult {
  return { json: rows, table: renderTable(rows, WORKFLOW_REVISION_COLUMNS) };
}

export function workflowRevisionResult(row: WorkflowRevision): CommandResult {
  return { json: row, table: renderTable([row], WORKFLOW_REVISION_COLUMNS) };
}

const WORKFLOW_GOAL_COLUMNS = [
  { header: 'ID', value: (row: WorkflowGoal) => row.id },
  { header: 'NAME', value: (row: WorkflowGoal) => row.name },
  { header: 'WORKFLOW REVISION', value: (row: WorkflowGoal) => row.workflowRevisionId },
  { header: 'PLAN', value: (row: WorkflowGoal) => row.planRevisionId },
  { header: 'PLAN DIGEST', value: (row: WorkflowGoal) => row.planDigest },
  { header: 'CREATED AT', value: (row: WorkflowGoal) => row.createdAt },
] as const;

export function workflowGoalsResult(rows: WorkflowGoal[]): CommandResult {
  return { json: rows, table: renderTable(rows, WORKFLOW_GOAL_COLUMNS) };
}

export function workflowGoalResult(row: WorkflowGoal): CommandResult {
  return { json: row, table: renderTable([row], WORKFLOW_GOAL_COLUMNS) };
}

export function workflowGoalMutationResult(row: WorkflowGoalMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.goal.id },
        { header: 'NAME', value: (value) => value.goal.name },
        { header: 'PLAN', value: (value) => value.planRevision.id },
        { header: 'PLAN DIGEST', value: (value) => value.planRevision.digest },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

export function workflowPlanRevisionResult(row: WorkflowPlanRevision): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.id },
        { header: 'GOAL', value: (value) => value.workflowGoalId },
        { header: 'COMPILER', value: (value) => value.compilerRevision },
        { header: 'DIGEST', value: (value) => value.digest },
        { header: 'STEPS', value: (value) => value.plan.steps.length },
        { header: 'CREATED AT', value: (value) => value.createdAt },
      ]
    ),
  };
}
