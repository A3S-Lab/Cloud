import type {
  HumanTask,
  HumanTaskMutationResult,
  HumanTaskSummary,
  WorkflowDefinition,
  WorkflowDefinitionMutationResult,
  WorkflowGoal,
  WorkflowGoalMutationResult,
  WorkflowNodeCatalog,
  WorkflowNodeCatalogEntry,
  WorkflowPlanRevision,
  WorkflowRevision,
  WorkflowRevisionSummary,
  WorkflowRun,
  WorkflowRunDiagnostics,
  WorkflowRunHistoryPage,
  WorkflowRunMutationResult,
  WorkflowRunOutput,
  WorkflowRunVariableInspection,
} from '@a3s/cloud-client';
import { renderTable } from './output';
import type { CommandResult } from './results';

const WORKFLOW_NODE_COLUMNS = [
  { header: 'ID', value: (row: WorkflowNodeCatalogEntry) => row.capabilityId },
  { header: 'LABEL', value: (row: WorkflowNodeCatalogEntry) => row.label },
  { header: 'KIND', value: (row: WorkflowNodeCatalogEntry) => row.kind },
  { header: 'EXECUTION', value: (row: WorkflowNodeCatalogEntry) => row.executionClass },
  { header: 'OWNER', value: (row: WorkflowNodeCatalogEntry) => row.owner },
  { header: 'AVAILABILITY', value: (row: WorkflowNodeCatalogEntry) => row.availability },
  { header: 'GATE', value: (row: WorkflowNodeCatalogEntry) => `${row.gate} (${row.gateState})` },
  { header: 'PROFILES', value: (row: WorkflowNodeCatalogEntry) => row.semanticProfiles.join(', ') },
] as const;

export function workflowNodeCatalogResult(catalog: WorkflowNodeCatalog): CommandResult {
  return { json: catalog, table: renderTable(catalog.nodes, WORKFLOW_NODE_COLUMNS) };
}

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

const WORKFLOW_RUN_COLUMNS = [
  { header: 'ID', value: (row: WorkflowRun) => row.id },
  { header: 'GOAL', value: (row: WorkflowRun) => row.workflowGoalId },
  { header: 'PLAN', value: (row: WorkflowRun) => row.planRevisionId },
  { header: 'STATUS', value: (row: WorkflowRun) => row.status },
  { header: 'STEPS', value: (row: WorkflowRun) => row.steps.length },
  { header: 'UPDATED AT', value: (row: WorkflowRun) => row.updatedAt },
  { header: 'ERROR', value: (row: WorkflowRun) => row.error },
] as const;

export function workflowRunsResult(rows: WorkflowRun[]): CommandResult {
  return { json: rows, table: renderTable(rows, WORKFLOW_RUN_COLUMNS) };
}

export function workflowRunResult(row: WorkflowRun): CommandResult {
  return { json: row, table: renderTable([row], WORKFLOW_RUN_COLUMNS) };
}

export function workflowRunMutationResult(row: WorkflowRunMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.workflowRun.id },
        { header: 'STATUS', value: (value) => value.workflowRun.status },
        { header: 'OPERATION', value: (value) => value.workflowRun.operationId },
        { header: 'PLAN', value: (value) => value.workflowRun.planRevisionId },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}

export function workflowRunOutputResult(row: WorkflowRunOutput): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'RUN', value: (value) => value.workflowRunId },
        { header: 'DIGEST', value: (value) => value.outputDigest },
        { header: 'FINISHED AT', value: (value) => value.finishedAt },
        { header: 'OUTPUT', value: (value) => JSON.stringify(value.output) },
      ]
    ),
  };
}

export function workflowRunHistoryResult(page: WorkflowRunHistoryPage): CommandResult {
  return {
    json: page,
    table: renderTable(page.events, [
      { header: 'SEQUENCE', value: (event) => event.sequence },
      { header: 'EVENT', value: (event) => event.eventKey },
      { header: 'STEP', value: (event) => event.stepId },
      { header: 'ATTEMPT', value: (event) => event.attempt },
      { header: 'OCCURRED AT', value: (event) => event.occurredAt },
    ]),
  };
}

export function workflowRunDiagnosticsResult(diagnostics: WorkflowRunDiagnostics): CommandResult {
  return {
    json: diagnostics,
    table: renderTable(
      [diagnostics],
      [
        { header: 'RUN', value: (value) => value.workflowRunId },
        { header: 'RUN STATUS', value: (value) => value.runStatus },
        { header: 'FLOW STATUS', value: (value) => value.observedFlowStatus },
        { header: 'DIAGNOSTICS', value: (value) => value.diagnosticStatus },
        { header: 'PROJECTED SEQUENCE', value: (value) => value.projectedFlowSequence },
        { header: 'OBSERVED SEQUENCE', value: (value) => value.observedFlowSequence },
        { header: 'UNPROJECTED', value: (value) => value.unprojectedEventCount },
        { header: 'EVENTS', value: (value) => value.flowStatistics.eventCount },
        { header: 'RETRIES', value: (value) => value.flowStatistics.retryEventCount },
        { header: 'EVIDENCE', value: (value) => value.stepStatistics.evidenceReferenceCount },
      ]
    ),
  };
}

export function workflowRunVariablesResult(inspection: WorkflowRunVariableInspection): CommandResult {
  return {
    json: inspection,
    table: renderTable(inspection.variables, [
      { header: 'NAME', value: (variable) => variable.name },
      { header: 'SCOPE', value: (variable) => variable.scope },
      { header: 'TYPE', value: (variable) => variable.valueType },
      { header: 'STORAGE', value: (variable) => variable.storageClass },
      { header: 'STATE', value: (variable) => variable.state },
      { header: 'REDACTED', value: (variable) => variable.redacted },
      { header: 'SOURCE STEP', value: (variable) => variable.sourceStepId },
      { header: 'VALUE', value: (variable) => JSON.stringify(variable.value) },
    ]),
  };
}

const HUMAN_TASK_COLUMNS = [
  { header: 'ID', value: (row: HumanTaskSummary) => row.id },
  { header: 'RUN', value: (row: HumanTaskSummary) => row.workflowRunId },
  { header: 'STEP', value: (row: HumanTaskSummary) => row.stepId },
  { header: 'STATUS', value: (row: HumanTaskSummary) => row.status },
  { header: 'CLAIMED BY', value: (row: HumanTaskSummary) => row.claimedBy },
  { header: 'DUE AT', value: (row: HumanTaskSummary) => row.dueAt },
  { header: 'UPDATED AT', value: (row: HumanTaskSummary) => row.updatedAt },
] as const;

export function humanTasksResult(rows: HumanTaskSummary[]): CommandResult {
  return { json: rows, table: renderTable(rows, HUMAN_TASK_COLUMNS) };
}

export function humanTaskResult(row: HumanTask): CommandResult {
  return { json: row, table: renderTable([row], HUMAN_TASK_COLUMNS) };
}

export function humanTaskMutationResult(row: HumanTaskMutationResult): CommandResult {
  return {
    json: row,
    table: renderTable(
      [row],
      [
        { header: 'ID', value: (value) => value.humanTask.id },
        { header: 'STATUS', value: (value) => value.humanTask.status },
        { header: 'CLAIMED BY', value: (value) => value.humanTask.claimedBy },
        { header: 'VERSION', value: (value) => value.humanTask.aggregateVersion },
        { header: 'REPLAYED', value: (value) => value.replayed },
      ]
    ),
  };
}
