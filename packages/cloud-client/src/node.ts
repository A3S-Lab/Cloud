export type NodeState = 'pending' | 'ready' | 'draining' | 'revoked';
export type NodeAvailability = 'online' | 'offline';

export interface Node {
  id: string;
  organizationId: string;
  name: string;
  state: NodeState;
  availability: NodeAvailability;
  agentInstanceId: string;
  agentVersion: string;
  runtimeProviderId: string;
  runtimeProviderBuild: string;
  capabilitiesDigest: string;
  capabilities: Record<string, unknown>;
  enrolledAt: string;
  lastObservedAt: string;
  aggregateVersion: number;
  replayed: boolean;
}

export interface EnrollmentToken {
  id: string;
  organizationId: string;
  name: string;
  aggregateVersion: number;
  createdAt: string;
  expiresAt: string;
  usedAt: string | null;
  revokedAt: string | null;
  replayed: boolean;
}

export interface IssueEnrollmentTokenInput {
  name: string;
  token: string;
  expiresAt: string;
}
