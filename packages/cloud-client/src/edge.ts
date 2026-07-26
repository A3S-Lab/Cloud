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
