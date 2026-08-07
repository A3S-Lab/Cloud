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

export type McpCredentialState = 'active' | 'expired' | 'revoked';

export interface McpCredential {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  prefix: string;
  state: McpCredentialState;
  generation: number;
  aggregateVersion: number;
  expiresAt: string;
  createdAt: string;
  updatedAt: string;
  revokedAt: string | null;
}

export interface CreateMcpCredentialInput {
  expiresAt: string;
}

export interface RotateMcpCredentialInput {
  expiresAt: string;
  expectedAggregateVersion: number;
}

export interface RevokeMcpCredentialInput {
  expectedAggregateVersion: number;
}

export interface McpCredentialDeliveryResult {
  credential: McpCredential;
  bearerCredential: string;
  deliveryExpiresAt: string;
  replayed: boolean;
}

export interface McpCredentialMutationResult {
  credential: McpCredential;
  replayed: boolean;
}

export interface McpRoutePolicyLimit {
  maxConcurrentRequests: number;
  requestsPerMinute: number;
  requestBurst: number;
}

export interface McpRoutePolicyGrant {
  credentialId: string;
  credentialGeneration: number;
  methods: string[];
  names: string[];
  limits: McpRoutePolicyLimit;
}

export interface McpRoutePolicy {
  id: string;
  organizationId: string;
  projectId: string;
  environmentId: string;
  gatewayScopeId: string;
  domainClaimId: string;
  workloadId: string;
  assetId: string;
  assetReleaseId: string;
  profileDigest: string;
  hostname: string;
  path: string;
  tlsRequired: boolean;
  allowedOrigins: string[];
  maxHeaderBytes: number;
  maxRequestBytes: number;
  maxResponseBytes: number;
  firstResponseTimeoutSeconds: number;
  streamIdleTimeoutSeconds: number;
  streamTotalTimeoutSeconds: number;
  drainTimeoutSeconds: number;
  telemetryNames: string[];
  telemetryEventsPerMinute: number;
  auditRequired: boolean;
  grants: McpRoutePolicyGrant[];
  policyRevision: number;
  policyDigest: string;
  acl: string;
  expiresAt: string;
  createdAt: string;
  updatedAt: string;
}

export interface McpRoutePolicyMutationResult {
  policy: McpRoutePolicy;
  replayed: boolean;
}
