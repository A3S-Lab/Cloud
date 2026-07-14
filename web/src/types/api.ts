export interface ApiEnvelope<T> {
  code: number;
  message: string;
  data: T;
  requestId: string;
  timestamp: string;
}

export interface ApiErrorEnvelope {
  code: number;
  statusCode: string;
  message: string;
  details: Record<string, unknown>;
  requestId: string;
  timestamp: string;
}

export interface Organization {
  id: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
}

export interface Project {
  organizationId: string;
  id: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
}

export interface Environment {
  organizationId: string;
  projectId: string;
  id: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
}

export type OperationStatus = 'queued' | 'running' | 'suspended' | 'succeeded' | 'failed' | 'cancelled';

export interface Operation {
  id: string;
  organizationId: string;
  subjectKind: string;
  subjectId: string;
  workflowName: string;
  workflowVersion: string;
  status: OperationStatus;
  lastSequence: number;
  requestedAt: string;
  updatedAt: string;
  error: string | null;
}
