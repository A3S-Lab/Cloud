import type { FormCanonicalValue, FormReleaseRef } from './form';

export type WorkflowPayloadKind = 'configuration' | 'data_schema' | 'policy';

export interface WorkflowPayloadAclInput {
  kind: WorkflowPayloadKind;
  acl: string;
}

export interface PublishWorkflowDefinitionInput {
  definitionAcl: string;
  payloads: WorkflowPayloadAclInput[];
}

export interface WorkflowDefinition {
  organizationId: string;
  projectId: string;
  id: string;
  name: string;
  description: string;
  currentRevisionId: string;
  currentRevisionNumber: number;
  currentRevisionDigest: string;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
}

export interface WorkflowPayload {
  kind: WorkflowPayloadKind;
  schema: 'cloud.workflow.configuration.v1' | 'cloud.workflow.data-schema.v1' | 'cloud.workflow.policy.v1';
  digest: string;
  canonicalAcl: string;
}

export interface WorkflowRevisionSummary {
  organizationId: string;
  projectId: string;
  workflowDefinitionId: string;
  id: string;
  revisionNumber: number;
  parentRevisionId: string | null;
  parentDigest: string | null;
  contractSchema: 'cloud.workflow.definition.v1';
  compilerSchemaVersion: number;
  contentDigest: string;
  payloadSetDigest: string;
  payloadCount: number;
  createdBy: string;
  createdAt: string;
}

export interface WorkflowRevision extends WorkflowRevisionSummary {
  canonicalDefinitionAcl: string;
  payloads: WorkflowPayload[];
}

export interface WorkflowDefinitionMutationResult {
  workflowDefinition: WorkflowDefinition;
  revision: WorkflowRevision;
  replayed: boolean;
}

export interface ReviseWorkflowDefinitionOptions {
  expectedVersion: number;
}

export type WorkflowStepKind =
  | 'input'
  | 'output'
  | 'transform'
  | 'branch'
  | 'human_decision'
  | 'execution'
  | 'agent'
  | 'mcp'
  | 'model'
  | 'tool'
  | 'service'
  | 'memory'
  | 'subworkflow';

export type WorkflowCapabilityOwner = 'assets' | 'workflow' | 'inference' | 'use' | 'executions';
export type WorkflowCapabilityType =
  | 'agent_release'
  | 'mcp_service_profile'
  | 'workflow_revision'
  | 'model_revision'
  | 'use_package'
  | 'execution_template'
  | 'connector_revision';

export interface WorkflowCapabilityReference {
  owner: WorkflowCapabilityOwner;
  type: WorkflowCapabilityType;
  resourceId: string;
  revision: string;
  digest: string;
  capability: string;
}

export interface WorkflowPlanStep {
  id: string;
  kind: WorkflowStepKind;
  configurationDigest: string;
  inputSchemaDigest: string;
  outputSchemaDigest: string;
  policyDigest: string | null;
  capability: WorkflowCapabilityReference | null;
}

export interface WorkflowPlanEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle: string | null;
}

export interface WorkflowPlan {
  schema: 'cloud.workflow.plan.v1';
  compilerRevision: 'cloud.workflow.plan-compiler.v1';
  workflowDefinitionId: string;
  workflowRevisionId: string;
  workflowDigest: string;
  workflowPayloadSetDigest: string;
  ontologyId: string;
  ontologyRevisionId: string;
  ontologyDigest: string;
  environmentId: string | null;
  inputDigest: string;
  steps: WorkflowPlanStep[];
  edges: WorkflowPlanEdge[];
}

export interface WorkflowPlanRevision {
  organizationId: string;
  projectId: string;
  workflowGoalId: string;
  id: string;
  schema: 'cloud.workflow.plan.v1';
  compilerRevision: 'cloud.workflow.plan-compiler.v1';
  digest: string;
  canonicalPlan: string;
  plan: WorkflowPlan;
  createdBy: string;
  createdAt: string;
}

export interface WorkflowGoal {
  organizationId: string;
  projectId: string;
  id: string;
  name: string;
  contractSchema: 'cloud.workflow.goal.v1';
  contractDigest: string;
  inputDigest: string;
  canonicalGoalAcl: string;
  workflowDefinitionId: string;
  workflowRevisionId: string;
  workflowDigest: string;
  ontologyId: string;
  ontologyRevisionId: string;
  ontologyDigest: string;
  environmentId: string | null;
  input: unknown;
  planRevisionId: string;
  planDigest: string;
  createdBy: string;
  createdAt: string;
}

export interface WorkflowGoalMutationResult {
  goal: WorkflowGoal;
  planRevision: WorkflowPlanRevision;
  replayed: boolean;
}

export type WorkflowRunStatus =
  | 'pending'
  | 'running'
  | 'waiting'
  | 'cancelling'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'timed_out';

export type WorkflowStepProjectionStatus =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'skipped';

export interface StartWorkflowRunInput {
  workflowGoalId: string;
  planRevisionId: string;
  timeoutSeconds?: number;
}

export interface CancelWorkflowRunInput {
  reason?: string;
}

export interface ListWorkflowRunsOptions {
  limit?: number;
}

export interface WaitWorkflowRunOptions {
  timeoutSeconds?: number;
}

export interface WorkflowRunHistoryOptions {
  afterSequence?: number;
  limit?: number;
}

export interface WorkflowStepProjection {
  stepId: string;
  kind: 'input' | 'transform' | 'branch' | 'output';
  status: WorkflowStepProjectionStatus;
  flowStepId: string;
  attemptGeneration: number;
  selectedHandle: string | null;
  result: unknown | null;
  resultDigest: string | null;
  error: string | null;
  evidenceReferences: string[];
  lastFlowSequence: number;
  updatedAt: string;
}

export interface WorkflowRun {
  organizationId: string;
  projectId: string;
  id: string;
  workflowGoalId: string;
  planRevisionId: string;
  planDigest: string;
  operationId: string;
  flowRunId: string;
  flowRuntimeBuildId: string | null;
  executionInputDigest: string;
  status: WorkflowRunStatus;
  lastFlowSequence: number;
  outputDigest: string | null;
  error: string | null;
  aggregateVersion: number;
  requestedBy: string;
  requestedAt: string;
  updatedAt: string;
  startedAt: string | null;
  deadlineAt: string;
  cancellationRequestedAt: string | null;
  cancellationReason: string | null;
  finishedAt: string | null;
  steps: WorkflowStepProjection[];
}

export interface WorkflowRunMutationResult {
  workflowRun: WorkflowRun;
  replayed: boolean;
}

export interface WorkflowRunOutput {
  workflowRunId: string;
  output: unknown;
  outputDigest: string;
  finishedAt: string;
}

export interface WorkflowRunHistoryEvent {
  sequence: number;
  eventId: string;
  eventKey: string;
  occurredAt: string;
  stepId: string | null;
  attempt: number | null;
  details: unknown;
}

export interface WorkflowRunHistoryPage {
  events: WorkflowRunHistoryEvent[];
  nextSequence: number | null;
}

export type HumanTaskStatus =
  | 'pending_activation'
  | 'ready'
  | 'claimed'
  | 'completed'
  | 'expired'
  | 'cancelled';

export type HumanTaskInteractionOutcome = 'submit' | 'approve' | 'reject';

export type HumanTaskInteractionOutputMapping =
  | { kind: 'identity' }
  | { kind: 'registry'; registryKey: string; revision: number; digest: string };

export interface HumanTaskAssignmentPolicy {
  id: string;
  revision: number;
  digest: string;
}

export interface HumanTaskInteractionIdentity {
  workflowRunId: string;
  flowRunId: string;
  stepId: string;
  stepAttempt: number;
  humanTaskId: string;
  flowHookId: string;
}

export interface HumanTaskInteractionAssignment {
  policyId: string;
  policyRevision: number;
  policyDigest: string;
  claimedPrincipalId: string;
}

export interface HumanTaskInteractionBinding {
  version: number;
  createdAt: string;
  dueAt?: string;
  expiresAt?: string;
}

export interface HumanTaskInteractionRequest {
  apiVersion: 'a3s.dev/form-interaction-request/v1';
  requestId: string;
  identity: HumanTaskInteractionIdentity;
  form: FormReleaseRef;
  assignment: HumanTaskInteractionAssignment;
  task: HumanTaskInteractionBinding;
  allowedOutcomes: HumanTaskInteractionOutcome[];
  outputMapping: HumanTaskInteractionOutputMapping;
  maxValueBytes: number;
  initialValue?: Record<string, unknown>;
  digest: string;
}

export interface HumanTaskInteractionSubmissionAssignment {
  policyId: string;
  policyRevision: number;
  policyDigest: string;
}

export interface HumanTaskInteractionSubmission {
  apiVersion: 'a3s.dev/form-interaction-submission/v1';
  submissionId: string;
  requestId: string;
  requestDigest: string;
  identity: HumanTaskInteractionIdentity;
  form: FormReleaseRef;
  assignment: HumanTaskInteractionSubmissionAssignment;
  taskVersion: number;
  principalId: string;
  outcome: HumanTaskInteractionOutcome;
  idempotencyKey: string;
  submittedAt: string;
  value: Readonly<Record<string, FormCanonicalValue>>;
  valueDigest: string;
}

export interface HumanTaskSummary {
  organizationId: string;
  projectId: string;
  id: string;
  workflowRunId: string;
  stepId: string;
  stepAttempt: number;
  formRelease: FormReleaseRef;
  assignmentPolicy: HumanTaskAssignmentPolicy;
  status: HumanTaskStatus;
  claimedBy: string | null;
  decisionId: string | null;
  aggregateVersion: number;
  message: string;
  allowedOutcomes: HumanTaskInteractionOutcome[];
  createdAt: string;
  updatedAt: string;
  dueAt: string | null;
  expiresAt: string | null;
  claimedAt: string | null;
  terminalAt: string | null;
}

export interface HumanTask extends HumanTaskSummary {
  details: string | null;
  outputMapping: HumanTaskInteractionOutputMapping;
  maxValueBytes: number;
  initialValue: Record<string, unknown> | null;
  interactionRequest: HumanTaskInteractionRequest | null;
}

export interface HumanTaskMutationResult {
  humanTask: HumanTask;
  replayed: boolean;
}

export interface ListHumanTasksOptions {
  status?: HumanTaskStatus;
  limit?: number;
}
