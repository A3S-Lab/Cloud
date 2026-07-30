export type RouteState = 'pending' | 'publishing' | 'active' | 'rejected' | 'unavailable';

export interface Route {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  gatewayScopeId: string;
  gatewayNodeId: string;
  hostname: string;
  pathPrefix: string;
  domainClaimId: string | null;
  domainPattern: string | null;
  gatewayCertificateId: string | null;
  workloadId: string;
  workloadRevisionId: string;
  runtimeUnitId: string;
  runtimeGeneration: number;
  portName: string;
  upstreamOrigin: string;
  targetObservedAt: string;
  state: RouteState;
  gatewayRevision: number | null;
  gatewayCommandId: string | null;
  snapshotDigest: string | null;
  failure: string | null;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  activatedAt: string | null;
}

export type GatewayCertificateState = 'provisioning' | 'issued' | 'ready' | 'failed' | 'revoked';

export interface GatewayCertificate {
  id: string;
  organizationId: string;
  nodeId: string;
  domainClaimIds: string[];
  dnsNames: string[];
  gatewayRevision: number;
  gatewayCommandId: string;
  snapshotDigest: string;
  state: GatewayCertificateState;
  serialNumber: string | null;
  fingerprint: string | null;
  issuedAt: string | null;
  expiresAt: string | null;
  failure: string | null;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  readyAt: string | null;
  revokedAt: string | null;
}

export type DomainClaimState = 'pending' | 'verified' | 'rejected' | 'revoked';

export interface DomainClaim {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  pattern: string;
  challengeDnsName: string;
  challengeValue: string;
  state: DomainClaimState;
  failure: string | null;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
  verifiedAt: string | null;
  revokedAt: string | null;
}

export interface DomainClaimMutationResult extends DomainClaim {
  replayed: boolean;
}

export interface GatewayScope {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  nodeId: string;
  memberNodeIds: string[];
  membershipGeneration: number;
  minReady: number;
  maxUnavailable: number;
  aggregateVersion: number;
  createdAt: string;
  updatedAt: string;
}

export interface GatewayScopeMutationResult extends GatewayScope {
  replayed: boolean;
}

export interface McpCredential {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  prefix: string;
  generation: number;
  aggregateVersion: number;
  expiresAt: string;
  createdAt: string;
  updatedAt: string;
  revokedAt: string | null;
}

export interface McpCredentialMutationResult extends McpCredential {
  replayed: boolean;
}

/**
 * Issuance/rotation response whose secret is recoverable only during the
 * server's bounded idempotency window. Callers must not persist it in logs.
 */
export interface McpCredentialDeliveryResult extends McpCredentialMutationResult {
  secret: string;
}

export interface McpCredentialExpiryInput {
  expiresAt: string;
}

export interface CreateGatewayScopeInput {
  nodeIds: string[];
  minReady: number;
  maxUnavailable: number;
}

export interface PublishRouteInput {
  gatewayScopeId: string;
  workloadRevisionId: string;
  domainClaimId: string;
  hostname: string;
  pathPrefix: string;
  portName: string;
}

export interface RoutePublicationResult {
  route: Route;
  certificate: GatewayCertificate;
  replayed: boolean;
  commandReplayed: boolean;
}
