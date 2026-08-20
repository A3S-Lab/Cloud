export const MAX_APPLICATION_RELEASE_ACL_BYTES = 64 * 1024;
export const MAX_APPLICATION_DESCRIPTION_CHARACTERS = 4_096;
export const DEFAULT_APPLICATION_LIST_LIMIT = 50;
export const MAX_APPLICATION_LIST_LIMIT = 200;

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
