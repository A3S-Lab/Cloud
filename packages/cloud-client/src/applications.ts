export const MAX_APPLICATION_RELEASE_ACL_BYTES = 64 * 1024;
export const MAX_APPLICATION_DESCRIPTION_CHARACTERS = 4_096;
export const DEFAULT_APPLICATION_LIST_LIMIT = 50;
export const MAX_APPLICATION_LIST_LIMIT = 200;
export const MAX_APPLICATION_CONVERSATION_VARIABLES_BYTES = 256 * 1024;
export const MAX_APPLICATION_INVOCATION_INPUT_BYTES = 64 * 1024;
export const DEFAULT_APPLICATION_MESSAGE_LIST_LIMIT = 100;
export const MAX_APPLICATION_MESSAGE_LIST_LIMIT = 500;
export const DEFAULT_APPLICATION_INVOCATION_TIMEOUT_SECONDS = 24 * 60 * 60;
export const MAX_APPLICATION_INVOCATION_TIMEOUT_SECONDS = 30 * 24 * 60 * 60;

export type ApplicationExperience =
  | 'chatbot'
  | 'text_generator'
  | 'classic_agent'
  | 'new_agent'
  | 'chatflow'
  | 'workflow';

export type ApplicationAudience = 'project_members' | 'authenticated_end_users' | 'anonymous';
export type ApplicationInteractionMode = 'conversation' | 'invocation';
export type ApplicationResponseMode = 'asynchronous' | 'blocking' | 'streaming';

export interface Application {
  organizationId: string;
  projectId: string;
  applicationId: string;
  name: string;
  description: string;
  experience: ApplicationExperience;
  currentReleaseId: string;
  currentReleaseNumber: number;
  currentReleaseDigest: string;
  aggregateVersion: number;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
}

export interface ApplicationRelease {
  organizationId: string;
  projectId: string;
  applicationId: string;
  releaseId: string;
  releaseNumber: number;
  parentReleaseId: string | null;
  parentDigest: string | null;
  experience: ApplicationExperience;
  audience: ApplicationAudience;
  interactionMode: ApplicationInteractionMode;
  responseModes: ApplicationResponseMode[];
  contractSchema: 'cloud.application.release.v1';
  contractAcl: string;
  contractDigest: string;
  workflowDefinitionId: string;
  workflowRevisionId: string;
  workflowContractDigest: string;
  workflowPayloadSetDigest: string;
  workflowSemanticContractSetDigest: string;
  inputSchemaDigest: string;
  outputSchemaDigest: string;
  presentationDigest: string;
  createdBy: string;
  createdAt: string;
}

export interface ApplicationRecord {
  application: Application;
  release: ApplicationRelease;
}

export interface ApplicationMutationResult {
  record: ApplicationRecord;
  replayed: boolean;
}

export interface CreateApplicationInput {
  name: string;
  description?: string;
  releaseAcl: string;
}

export interface PublishApplicationReleaseInput {
  expectedVersion: number;
  releaseAcl: string;
}

export type ApplicationSessionStatus = 'active' | 'closed';
export type ApplicationInvocationStatus =
  | 'requested'
  | 'running'
  | 'cancelling'
  | 'succeeded'
  | 'failed'
  | 'cancelled';
export type ApplicationMessageKind = 'input' | 'answer' | 'final_output';

export interface ApplicationSession {
  organizationId: string;
  projectId: string;
  applicationId: string;
  applicationReleaseId: string;
  applicationReleaseNumber: number;
  applicationReleaseDigest: string;
  endUserId: string;
  sessionId: string;
  interactionMode: ApplicationInteractionMode;
  status: ApplicationSessionStatus;
  lastMessageSequence: number;
  currentVariableRevisionId: string;
  currentVariableRevisionNumber: number;
  currentVariableDigest: string;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  closedAt: string | null;
}

export interface ApplicationSessionMutationResult {
  session: ApplicationSession;
  replayed: boolean;
}

export interface ApplicationExpectedVersionInput {
  expectedVersion: number;
}

export interface OpenApplicationSessionInput {
  releaseId: string;
  initialVariables?: Record<string, unknown>;
}

export interface ApplicationInvocation {
  organizationId: string;
  projectId: string;
  applicationId: string;
  applicationReleaseId: string;
  applicationReleaseDigest: string;
  sessionId: string;
  invocationId: string;
  responseMode: ApplicationResponseMode;
  input: Record<string, unknown>;
  inputDigest: string;
  workflowRunId: string | null;
  status: ApplicationInvocationStatus;
  aggregateVersion: number;
  requestedAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface ApplicationWorkflowRunEvidence {
  workflowRunId: string;
  workflowGoalId: string;
  planRevisionId: string;
  planDigest: string;
  ontologyId: string;
  ontologyRevisionId: string;
  ontologyDigest: string;
  environmentId: string | null;
  requestedAt: string;
  deadlineAt: string;
}

export interface ApplicationInvocationMutationResult {
  invocation: ApplicationInvocation;
  workflow: ApplicationWorkflowRunEvidence;
  replayed: boolean;
}

export interface ApplicationInvocationCancellationResult {
  invocation: ApplicationInvocation;
  workflow: ApplicationWorkflowRunEvidence | null;
  replayed: boolean;
}

export interface RequestApplicationInvocationInput {
  ontologyId: string;
  ontologyRevisionId: string;
  environmentId?: string;
  responseMode: ApplicationResponseMode;
  input: Record<string, unknown>;
  timeoutSeconds?: number;
}

export interface ApplicationWorkflowEffect {
  workflowRunId: string;
  stepId: string;
  attempt: number;
  ordinal: number;
}

export interface ApplicationMessage {
  organizationId: string;
  projectId: string;
  applicationId: string;
  applicationReleaseId: string;
  applicationReleaseDigest: string;
  sessionId: string;
  invocationId: string;
  messageId: string;
  sequence: number;
  kind: ApplicationMessageKind;
  content: unknown;
  contentDigest: string;
  workflowEffect: ApplicationWorkflowEffect | null;
  createdAt: string;
}

export interface ApplicationConversationVariables {
  organizationId: string;
  projectId: string;
  applicationId: string;
  applicationReleaseId: string;
  applicationReleaseDigest: string;
  sessionId: string;
  revisionId: string;
  revisionNumber: number;
  parentRevisionId: string | null;
  parentDigest: string | null;
  values: Record<string, unknown>;
  valuesDigest: string;
  sourceEffect: ApplicationWorkflowEffect | null;
  createdAt: string;
}

export interface ApplicationSessionReplay {
  session: ApplicationSession;
  messages: ApplicationMessage[];
  currentVariables: ApplicationConversationVariables;
  nextSequence: number;
  hasMore: boolean;
}

export function validateApplicationName(name: string): void {
  const normalized = name.trim();
  if (
    normalized.length === 0 ||
    Array.from(normalized).length > 63 ||
    normalized.includes('\0') ||
    normalized.includes('\r') ||
    normalized.includes('\n')
  ) {
    throw new RangeError('Application name must contain 1 to 63 visible characters');
  }
}

export function validateApplicationDescription(description: string): void {
  if (
    Array.from(description).length > MAX_APPLICATION_DESCRIPTION_CHARACTERS ||
    description.includes('\0') ||
    description.includes('\r')
  ) {
    throw new RangeError(
      `Application description must contain at most ${MAX_APPLICATION_DESCRIPTION_CHARACTERS} characters`
    );
  }
}

export function validateApplicationReleaseAcl(acl: string): void {
  const byteLength = new TextEncoder().encode(acl).byteLength;
  if (
    byteLength < 1 ||
    byteLength > MAX_APPLICATION_RELEASE_ACL_BYTES ||
    acl.replaceAll('\r\n', '').includes('\r')
  ) {
    throw new RangeError(
      `Application release ACL must contain between 1 and ${MAX_APPLICATION_RELEASE_ACL_BYTES} UTF-8 bytes without bare carriage returns`
    );
  }
}

export function validateApplicationExpectedVersion(expectedVersion: number): void {
  if (!Number.isSafeInteger(expectedVersion) || expectedVersion < 1) {
    throw new RangeError('expected Application version must be a positive safe integer');
  }
}

export function validateApplicationListLimit(limit: number): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_APPLICATION_LIST_LIMIT) {
    throw new RangeError(`Application list limit must be between 1 and ${MAX_APPLICATION_LIST_LIMIT}`);
  }
}

export function validateApplicationInitialVariables(value: unknown): void {
  validateApplicationObject(
    value,
    'Application initial variables',
    MAX_APPLICATION_CONVERSATION_VARIABLES_BYTES
  );
}

export function validateApplicationInvocationInput(value: unknown): void {
  validateApplicationObject(value, 'Application invocation input', MAX_APPLICATION_INVOCATION_INPUT_BYTES);
}

export function validateApplicationResponseMode(value: string): void {
  if (!['asynchronous', 'blocking', 'streaming'].includes(value)) {
    throw new RangeError('Application response mode is unsupported');
  }
}

export function validateApplicationInvocationTimeout(timeoutSeconds: number): void {
  if (
    !Number.isSafeInteger(timeoutSeconds) ||
    timeoutSeconds < 1 ||
    timeoutSeconds > MAX_APPLICATION_INVOCATION_TIMEOUT_SECONDS
  ) {
    throw new RangeError(
      `Application invocation timeout must be between 1 and ${MAX_APPLICATION_INVOCATION_TIMEOUT_SECONDS} seconds`
    );
  }
}

export function validateApplicationMessageList(afterSequence: number, limit: number): void {
  if (!Number.isSafeInteger(afterSequence) || afterSequence < 0) {
    throw new RangeError('Application message afterSequence must be a non-negative safe integer');
  }
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_APPLICATION_MESSAGE_LIST_LIMIT) {
    throw new RangeError(
      `Application message list limit must be between 1 and ${MAX_APPLICATION_MESSAGE_LIST_LIMIT}`
    );
  }
}

function validateApplicationObject(value: unknown, label: string, maximumBytes: number): void {
  if (value === null || Array.isArray(value) || typeof value !== 'object') {
    throw new RangeError(`${label} must be a JSON object`);
  }
  let encoded: Uint8Array;
  try {
    const serialized = JSON.stringify(value);
    if (serialized === undefined) {
      throw new TypeError('JSON serialization returned no value');
    }
    encoded = new TextEncoder().encode(serialized);
  } catch {
    throw new RangeError(`${label} must be JSON serializable`);
  }
  if (encoded.byteLength > maximumBytes) {
    throw new RangeError(`${label} must contain at most ${maximumBytes} UTF-8 bytes`);
  }
}
