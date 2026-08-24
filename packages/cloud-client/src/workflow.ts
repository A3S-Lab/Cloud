import type { FormCanonicalValue, FormReleaseRef } from './form';

export type WorkflowPayloadKind = 'configuration' | 'data_schema' | 'policy';

export interface WorkflowPayloadAclInput {
  kind: WorkflowPayloadKind;
  acl: string;
}

export interface WorkflowSemanticContractAclsInput {
  descriptorBindingsAcl: string;
  descriptorRegistryAcl: string;
  variableContractAcl: string;
  variableDefaultsAcl?: string;
  compositeRegionsAcl?: string;
}

export interface PublishWorkflowDefinitionInput {
  definitionAcl: string;
  payloads: WorkflowPayloadAclInput[];
  semanticContracts?: WorkflowSemanticContractAclsInput;
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
  schema:
    | 'cloud.workflow.configuration.v1'
    | 'cloud.workflow.data-schema.v1'
    | 'cloud.workflow.policy.v1'
    | 'cloud.workflow.policy.v2'
    | 'cloud.workflow.policy.v3';
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
  semanticContractSetDigest: string | null;
  semanticContractCount: number;
  createdBy: string;
  createdAt: string;
}

export interface WorkflowRevision extends WorkflowRevisionSummary {
  canonicalDefinitionAcl: string;
  payloads: WorkflowPayload[];
  semanticContracts: WorkflowSemanticContract[];
}

export type WorkflowSemanticContractKind =
  | 'composite_regions'
  | 'descriptor_bindings'
  | 'descriptor_registry'
  | 'variable_contract'
  | 'variable_defaults';

export interface WorkflowSemanticContract {
  kind: WorkflowSemanticContractKind;
  schema:
    | 'cloud.workflow.composite-regions.v1'
    | 'cloud.workflow.step-descriptor-bindings.v1'
    | 'cloud.workflow.step-descriptor-registry.v1'
    | 'cloud.workflow.variable-contract.v1'
    | 'cloud.workflow.variable-defaults.v1';
  digest: string;
  canonicalAcl: string;
}

export interface WorkflowDefinitionMutationResult {
  workflowDefinition: WorkflowDefinition;
  revision: WorkflowRevision;
  replayed: boolean;
}

export interface ReviseWorkflowDefinitionOptions {
  expectedVersion: number;
}

export type WorkflowNodeCatalogAvailability = 'unavailable' | 'internal' | 'public';
export type WorkflowNodeExecutionClass =
  | 'workflow_local'
  | 'composite_region'
  | 'owning_application_port'
  | 'invocation_only';
export type WorkflowNodeGateState = 'planned' | 'in_progress' | 'implemented' | 'verified';
export type WorkflowNodeOwner =
  | 'agents'
  | 'applications'
  | 'assets'
  | 'automations'
  | 'connectors'
  | 'edge_gateway'
  | 'executions'
  | 'files'
  | 'identity'
  | 'inference'
  | 'knowledge'
  | 'operations_telemetry'
  | 'platform'
  | 'use'
  | 'workflow';

export interface WorkflowNodeCatalogEntry {
  capabilityId: string;
  label: string;
  owner: WorkflowNodeOwner;
  gate: string;
  gateState: WorkflowNodeGateState;
  dependencies: string[];
  availability: WorkflowNodeCatalogAvailability;
  kind: WorkflowStepKind | null;
  executionClass: WorkflowNodeExecutionClass;
  semanticProfiles: string[];
  evidence: string[];
  unavailableReason: string | null;
}

export interface WorkflowNodeCatalog {
  schema: 'a3s.cloud.app-platform.workflow-node-profiles.v1';
  revision: '1.0.0';
  baseline: string;
  parityManifestDigest: string;
  profileSetDigest: string;
  parityClaim: boolean;
  nodes: WorkflowNodeCatalogEntry[];
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
  descriptor: WorkflowStepDescriptorBinding | null;
  failure?: WorkflowStepFailureContract;
  defaultOutput?: WorkflowStepDefaultOutputContract;
}

export interface WorkflowStepDefaultOutputContract {
  outputPort: WorkflowStepPort;
}

export type WorkflowStepPortCardinality = 'single' | 'many';

export type WorkflowStepRetryClassification = 'not_retryable' | 'flow_retryable' | 'owner_classified';

export type WorkflowStepFallbackMode = 'unsupported' | 'default_output' | 'failure_branch';

export interface WorkflowStepPort {
  name: string;
  valueType: WorkflowDataType;
  cardinality: WorkflowStepPortCardinality;
  required: boolean;
  dynamic: boolean;
}

export interface WorkflowStepFailureContract {
  errorOutput: WorkflowStepPort | null;
  retryClassification: WorkflowStepRetryClassification;
  fallback: WorkflowStepFallbackMode;
  failureBranch: boolean;
}

export type WorkflowStepFailureClassification =
  | 'dispatch_rejected'
  | 'execution_failed'
  | 'execution_cancelled'
  | 'provider_rejected'
  | 'provider_attempts_exhausted'
  | 'provider_indeterminate'
  | 'provider_observation_limit'
  | 'provider_response_invalid'
  | 'application_invalid'
  | 'application_not_found'
  | 'application_conflict'
  | 'application_forbidden'
  | 'workflow_local_invalid';

export type WorkflowExecutionOutcome =
  | { kind: 'succeeded'; exit_code: 0 }
  | { kind: 'failed'; exit_code: number | null; reason: string }
  | { kind: 'cancelled' };

export interface WorkflowExecutionStepOutput {
  schema: 'cloud.workflow.execution-result.v1';
  executionId: string;
  operationId: string;
  executionTemplateId: string;
  executionTemplateRevisionId: string;
  executionTemplateDigest: string;
  invocationTemplateDigest: string;
  outcome: WorkflowExecutionOutcome;
  finishedAt: string;
}

export interface WorkflowExecutionFailureDetails {
  kind: 'execution';
  output: WorkflowExecutionStepOutput;
}

export interface WorkflowStepFailureOutput {
  schema:
    | 'cloud.workflow.step-failure.v1'
    | 'cloud.workflow.step-failure.v2'
    | 'cloud.workflow.step-failure.v3'
    | 'cloud.workflow.step-failure.v4'
    | 'cloud.workflow.step-failure.v5'
    | 'cloud.workflow.step-failure.v6'
    | 'cloud.workflow.step-failure.v7';
  stepId: string;
  classification: WorkflowStepFailureClassification;
  message: string;
  details?: WorkflowExecutionFailureDetails;
}

export interface WorkflowStepDefaultOutputEvidence {
  schema: 'cloud.workflow.step-default-output.v1';
  policyDigest: string;
  port: string;
  failure: WorkflowStepFailureOutput;
}

export interface WorkflowStepDescriptorBinding {
  stepId: string;
  descriptorId: string;
  descriptorRevision: string;
  semanticDigest: string;
}

export interface WorkflowPlanEdge {
  id: string;
  source: string;
  target: string;
  sourceHandle: string | null;
}

export interface WorkflowPlan {
  schema:
    | 'cloud.workflow.plan.v1'
    | 'cloud.workflow.plan.v2'
    | 'cloud.workflow.plan.v3'
    | 'cloud.workflow.plan.v4'
    | 'cloud.workflow.plan.v5'
    | 'cloud.workflow.plan.v6'
    | 'cloud.workflow.plan.v7'
    | 'cloud.workflow.plan.v8'
    | 'cloud.workflow.plan.v9'
    | 'cloud.workflow.plan.v10';
  compilerRevision:
    | 'cloud.workflow.plan-compiler.v1'
    | 'cloud.workflow.plan-compiler.v2'
    | 'cloud.workflow.plan-compiler.v3'
    | 'cloud.workflow.plan-compiler.v4'
    | 'cloud.workflow.plan-compiler.v5'
    | 'cloud.workflow.plan-compiler.v6'
    | 'cloud.workflow.plan-compiler.v7'
    | 'cloud.workflow.plan-compiler.v8'
    | 'cloud.workflow.plan-compiler.v9'
    | 'cloud.workflow.plan-compiler.v10';
  workflowDefinitionId: string;
  workflowRevisionId: string;
  workflowDigest: string;
  workflowPayloadSetDigest: string;
  semanticContractSetDigest: string | null;
  variableContractDigest: string | null;
  compositeRegionsDigest: string | null;
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
  schema:
    | 'cloud.workflow.plan.v1'
    | 'cloud.workflow.plan.v2'
    | 'cloud.workflow.plan.v3'
    | 'cloud.workflow.plan.v4'
    | 'cloud.workflow.plan.v5'
    | 'cloud.workflow.plan.v6'
    | 'cloud.workflow.plan.v7'
    | 'cloud.workflow.plan.v8'
    | 'cloud.workflow.plan.v9'
    | 'cloud.workflow.plan.v10';
  compilerRevision:
    | 'cloud.workflow.plan-compiler.v1'
    | 'cloud.workflow.plan-compiler.v2'
    | 'cloud.workflow.plan-compiler.v3'
    | 'cloud.workflow.plan-compiler.v4'
    | 'cloud.workflow.plan-compiler.v5'
    | 'cloud.workflow.plan-compiler.v6'
    | 'cloud.workflow.plan-compiler.v7'
    | 'cloud.workflow.plan-compiler.v8'
    | 'cloud.workflow.plan-compiler.v9'
    | 'cloud.workflow.plan-compiler.v10';
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

export type WorkflowStepEvidenceReference =
  | `urn:a3s:cloud:connectors:attempt:${string}`
  | `urn:a3s:cloud:executions:execution:${string}`
  | `urn:a3s:cloud:forms:submission:${string}`
  | `urn:a3s:cloud:operations:operation:${string}`
  | `urn:a3s:cloud:workflow:human-task:${string}`
  | `urn:a3s:cloud:workflow:workflow-decision:${string}`;

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
  kind: WorkflowStepKind;
  status: WorkflowStepProjectionStatus;
  flowStepId: string;
  attemptGeneration: number;
  selectedHandle: string | null;
  result: unknown | null;
  resultDigest: string | null;
  error: string | null;
  defaultOutputEvidence: WorkflowStepDefaultOutputEvidence | null;
  evidenceReferences: WorkflowStepEvidenceReference[];
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
  cancellationRequestedBy: string | null;
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

export type WorkflowDataType = 'any' | 'object' | 'array' | 'string' | 'number' | 'boolean' | 'null';

export type WorkflowVariableScope =
  | 'invocation_input'
  | 'node_output'
  | 'composite_local'
  | 'run'
  | 'application';

export type WorkflowVariableStorageClass = 'inline' | 'secret_reference' | 'immutable_object_reference';

export type WorkflowVariableMutationMode = 'immutable' | 'deterministic' | 'optimistic_application_port';

export type WorkflowRunVariableState = 'materialized' | 'unavailable';

export interface WorkflowRunVariable {
  name: string;
  scope: WorkflowVariableScope;
  valueType: WorkflowDataType;
  valueSchemaDigest: string;
  storageClass: WorkflowVariableStorageClass;
  mutationMode: WorkflowVariableMutationMode;
  required: boolean;
  sourceStepId: string | null;
  state: WorkflowRunVariableState;
  redacted: boolean;
  value: unknown;
  valueDigest: string | null;
}

export interface WorkflowRunVariableInspection {
  schema: 'cloud.workflow-run.variable-inspection.v1';
  workflowRunId: string;
  planRevisionId: string;
  variableContractDigest: string;
  lastFlowSequence: number;
  observedAt: string;
  variables: WorkflowRunVariable[];
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
