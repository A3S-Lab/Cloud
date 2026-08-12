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

export type NodePoolMaintenanceStatus = 'scheduled' | 'active' | 'completed' | 'cancelled';

export interface NodePoolMaintenance {
  generation: number;
  targetNodeIds: string[];
  startsAt: string;
  endsAt: string;
  reason: string;
  cancelledAt: string | null;
  status: NodePoolMaintenanceStatus;
}

export interface NodePool {
  id: string;
  organizationId: string;
  name: string;
  memberNodeIds: string[];
  maintenance: NodePoolMaintenance | null;
  specDigest: string;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  replayed: boolean;
}

export interface CreateNodePoolInput {
  name: string;
  memberNodeIds: string[];
}

export interface AddNodePoolMembersInput {
  expectedVersion: number;
  memberNodeIds: string[];
}

export interface ScheduleNodePoolMaintenanceInput {
  expectedVersion: number;
  targetNodeIds: string[];
  startsAt: string;
  endsAt: string;
  reason: string;
}

export interface CancelNodePoolMaintenanceInput {
  expectedVersion: number;
  maintenanceGeneration: number;
}
