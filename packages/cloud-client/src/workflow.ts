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
