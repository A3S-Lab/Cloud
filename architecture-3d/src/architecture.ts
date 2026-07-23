export type ArchitectureStatus = 'verified' | 'in-progress' | 'planned' | 'external';

export interface ArchitectureStatusMeta {
  label: string;
  description: string;
  color: string;
}

export const ARCHITECTURE_STATUS_META: Readonly<Record<ArchitectureStatus, ArchitectureStatusMeta>> = {
  verified: {
    label: 'Verified',
    description: 'Backed by repeatable release or conformance evidence.',
    color: '#b8f36b',
  },
  'in-progress': {
    label: 'In progress',
    description: 'Actively advancing through the current delivery gate.',
    color: '#f3c86b',
  },
  planned: {
    label: 'Planned',
    description: 'Sequenced after its required platform foundations.',
    color: '#72b7ff',
  },
  external: {
    label: 'External',
    description: 'Outside the Cloud ownership boundary.',
    color: '#91a398',
  },
};

export type ArchitectureLayerId = 'interfaces' | 'control' | 'state' | 'node' | 'provider';

export type JourneyId = 'all' | 'deploy' | 'source' | 'traffic' | 'observe';

export interface ArchitectureLayer {
  id: ArchitectureLayerId;
  label: string;
  description: string;
  y: number;
  color: string;
}

export interface ArchitectureNode {
  id: string;
  label: string;
  eyebrow: string;
  layer: ArchitectureLayerId;
  position: readonly [number, number, number];
  status: ArchitectureStatus;
  gate: string;
  summary: string;
  owns: readonly string[];
  boundary: string;
  docsUrl: string;
}

export interface ArchitectureEdge {
  id: string;
  from: string;
  to: string;
  label: string;
  journeys: readonly Exclude<JourneyId, 'all'>[];
}

export interface Journey {
  id: JourneyId;
  label: string;
  shortLabel: string;
  description: string;
  color: string;
}

export interface ArchitectureGraph {
  layers: readonly ArchitectureLayer[];
  nodes: readonly ArchitectureNode[];
  edges: readonly ArchitectureEdge[];
  journeys: readonly Journey[];
}

const cloudDocs = 'https://github.com/A3S-Lab/Cloud/blob/main';

export const ARCHITECTURE_LAYERS: readonly ArchitectureLayer[] = [
  {
    id: 'interfaces',
    label: 'Intent & interfaces',
    description: 'Human, automation, source, and inference entry points',
    y: 6,
    color: '#a9d8ff',
  },
  {
    id: 'control',
    label: 'Cloud control plane',
    description: 'Tenant-scoped desired state, reconciliation, and policy',
    y: 2.5,
    color: '#b8f36b',
  },
  {
    id: 'state',
    label: 'Durable coordination',
    description: 'Authoritative state, operation history, facts, and content',
    y: -1,
    color: '#d7b6ff',
  },
  {
    id: 'node',
    label: 'Managed node plane',
    description: 'Outbound control, provider-neutral execution, and traffic',
    y: -4.5,
    color: '#71d5c3',
  },
  {
    id: 'provider',
    label: 'Provider resources',
    description: 'Concrete build, runtime, registry, and workload resources',
    y: -8,
    color: '#f3c86b',
  },
] as const;

export const ARCHITECTURE_NODES: readonly ArchitectureNode[] = [
  {
    id: 'clients',
    label: 'Clients & SDKs',
    eyebrow: 'Request origin',
    layer: 'interfaces',
    position: [-8, 6, 2.4],
    status: 'external',
    gate: 'External',
    summary: 'Browsers, operators, automation, and OpenAI-compatible clients originate intent and traffic.',
    owns: ['User intent', 'API requests', 'Traffic payloads'],
    boundary: 'Clients never receive provider credentials or direct access to Cloud durable state.',
    docsUrl: `${cloudDocs}/docs/architecture.md`,
  },
  {
    id: 'web',
    label: 'Web Console',
    eyebrow: 'Management surface',
    layer: 'interfaces',
    position: [-4, 6, 0.4],
    status: 'verified',
    gate: 'F0 · E0',
    summary: 'The same-origin management SPA consumes authoritative command, query, operation, and log APIs.',
    owns: ['Operator experience', 'Status projections', 'Safe command initiation'],
    boundary: 'The console owns no business rule and cannot bypass tenant or idempotency guards.',
    docsUrl: `${cloudDocs}/docs/architecture.md#41-management-web-delivery`,
  },
  {
    id: 'control-surfaces',
    label: 'REST · CLI · MCP',
    eyebrow: 'Automation surfaces',
    layer: 'interfaces',
    position: [0.2, 6, 0.4],
    status: 'planned',
    gate: 'C0',
    summary: 'REST, CLI, and management MCP converge on the same application commands and queries.',
    owns: ['Stable automation contracts', 'Authorized search', 'Scoped management tools'],
    boundary: 'No presentation surface may introduce a second policy or orchestration path.',
    docsUrl: `${cloudDocs}/docs/development-plan.md`,
  },
  {
    id: 'github',
    label: 'GitHub Source',
    eyebrow: 'External provider',
    layer: 'interfaces',
    position: [4.5, 6, 1.8],
    status: 'in-progress',
    gate: 'G0',
    summary: 'Signed webhooks and short-lived GitHub App credentials resolve one exact source revision.',
    owns: ['Repository events', 'Installation authority', 'Immutable Git objects'],
    boundary: 'Mutable refs and credentials never become Cloud source truth.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#33-external-sources`,
  },
  {
    id: 'inference',
    label: 'Inference Profile',
    eyebrow: 'Optional product profile',
    layer: 'interfaces',
    position: [8.3, 6, -0.8],
    status: 'planned',
    gate: 'I0',
    summary: 'GPU-backed model serving reuses Workloads, Fleet, Edge, Identity, Artifacts, and Gateway.',
    owns: ['Model catalog', 'Typed backend plans', 'Model routes and usage'],
    boundary: 'Inference is not a second scheduler, deployment engine, or traffic proxy.',
    docsUrl: `${cloudDocs}/docs/inference-plan.md`,
  },
  {
    id: 'api',
    label: 'A3S Boot API',
    eyebrow: 'Control-plane boundary',
    layer: 'control',
    position: [-8.2, 2.5, 0],
    status: 'verified',
    gate: 'F0',
    summary:
      'Authenticated HTTP boundaries establish tenant context and dispatch typed commands and queries.',
    owns: ['Transport validation', 'Tenant context', 'Response contracts'],
    boundary: 'Controllers remain thin; provider details and business rules stay behind application ports.',
    docsUrl: `${cloudDocs}/docs/architecture.md#2-system-shape`,
  },
  {
    id: 'identity',
    label: 'Identity',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [-5.4, 2.5, 2],
    status: 'verified',
    gate: 'F0',
    summary: 'Organizations, memberships, tokens, grants, and the tenant security boundary.',
    owns: ['Organization', 'Membership', 'API token'],
    boundary: 'Identity decides who may issue a command, not where a workload runs.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#31-identity-and-access`,
  },
  {
    id: 'projects',
    label: 'Projects',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [-2.7, 2.5, 2],
    status: 'verified',
    gate: 'F0',
    summary: 'Projects and environments form the product and desired-state namespace hierarchy.',
    owns: ['Project', 'Environment', 'Attribution profile'],
    boundary: 'Environment deletion and other long work are durable operations, not hidden cascades.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#32-projects`,
  },
  {
    id: 'sources',
    label: 'Sources',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [0.1, 2.5, 2],
    status: 'in-progress',
    gate: 'G0',
    summary:
      'Provider authority and immutable external source revisions are accepted without durable credentials.',
    owns: ['GitHub connection', 'Subscription', 'External source revision'],
    boundary: 'Sources owns provider identity and revision truth, not builds or deployment.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#33-external-sources`,
  },
  {
    id: 'artifacts',
    label: 'Artifacts',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [2.9, 2.5, 2],
    status: 'in-progress',
    gate: 'G0',
    summary:
      'Isolated builds produce validated, signed, content-addressed OCI graphs and trusted retry caches.',
    owns: ['BuildRun', 'OCI descriptor', 'SBOM and provenance'],
    boundary: 'Every cache hit and publication is revalidated against immutable source and builder inputs.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#35-artifacts`,
  },
  {
    id: 'workloads',
    label: 'Workloads',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [-3.9, 2.5, -1.8],
    status: 'verified',
    gate: 'D0 · E0',
    summary:
      'One deployment abstraction owns revisions, placement intent, rollout, health, stop, and rollback.',
    owns: ['Workload', 'Workload revision', 'Deployment'],
    boundary: 'Applications, Agents, MCP services, and inference all reuse this path.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#37-workloads-and-deployments`,
  },
  {
    id: 'fleet',
    label: 'Fleet',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [-0.8, 2.5, -1.8],
    status: 'verified',
    gate: 'N0 · E0',
    summary: 'Node identity, capabilities, leases, observations, drain, and ordered log ingestion.',
    owns: ['Node', 'Command lease', 'Observation and log cursor'],
    boundary: 'Nodes connect outward and never receive PostgreSQL or NATS credentials.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#36-fleet`,
  },
  {
    id: 'edge',
    label: 'Edge',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [2.5, 2.5, -1.8],
    status: 'verified',
    gate: 'E0',
    summary: 'Domains, managed TLS, routes, complete Gateway snapshots, and exact acknowledgements.',
    owns: ['Domain claim', 'Route', 'Gateway snapshot'],
    boundary: 'Cloud owns complete policy; Gateway applies it atomically without inventing desired state.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#39-edge`,
  },
  {
    id: 'operations',
    label: 'Operations',
    eyebrow: 'Bounded context',
    layer: 'control',
    position: [5.7, 2.5, -1.8],
    status: 'verified',
    gate: 'F0 · E0',
    summary:
      'Durable operation projections expose long-running progress, cancellation, replay, and recovery.',
    owns: ['Operation record', 'Workflow identity', 'Timeline projection'],
    boundary: 'A business commit and a Flow run converge idempotently across their crash gap.',
    docsUrl: `${cloudDocs}/docs/domain-model.md#310-operations`,
  },
  {
    id: 'postgres',
    label: 'PostgreSQL',
    eyebrow: 'Authoritative state',
    layer: 'state',
    position: [-6, -1, 0.7],
    status: 'verified',
    gate: 'F0',
    summary: 'The source of truth for aggregates, desired state, idempotency, outbox, and UI projections.',
    owns: ['Business state', 'Optimistic versions', 'Transactional outbox'],
    boundary: 'Provider observations become durable facts before they can advance desired state.',
    docsUrl: `${cloudDocs}/docs/architecture.md#5-data-and-consistency-ownership`,
  },
  {
    id: 'flow',
    label: 'A3S Flow',
    eyebrow: 'Operation history',
    layer: 'state',
    position: [-2, -1, 0.7],
    status: 'verified',
    gate: 'F0 · E0',
    summary: 'Coordinates long-lived, replay-safe deployment, build, rollback, and repair workflows.',
    owns: ['Workflow event history', 'Durable step state', 'Retry coordination'],
    boundary: 'Flow coordinates side effects; it does not replace aggregate invariants.',
    docsUrl: `${cloudDocs}/docs/architecture.md#5-data-and-consistency-ownership`,
  },
  {
    id: 'event',
    label: 'A3S Event',
    eyebrow: 'Committed facts',
    layer: 'state',
    position: [2, -1, 0.7],
    status: 'verified',
    gate: 'F0',
    summary: 'The outbox relay publishes integration facts only after the originating transaction commits.',
    owns: ['Integration events', 'Outbox relay', 'Redelivery'],
    boundary: 'Events distribute facts but never repair an invariant inside the same transaction.',
    docsUrl: `${cloudDocs}/docs/architecture.md#2-system-shape`,
  },
  {
    id: 'object-storage',
    label: 'Artifact & Log Store',
    eyebrow: 'Content-addressed bytes',
    layer: 'state',
    position: [6, -1, 0.7],
    status: 'verified',
    gate: 'E0 · G0',
    summary: 'Immutable log chunks and command-bound Artifact archives live outside business rows.',
    owns: ['Verified object bytes', 'Artifact receipts', 'Retention lifecycle'],
    boundary: 'PostgreSQL stores descriptors and gaps, never unbounded image or log bodies.',
    docsUrl: `${cloudDocs}/docs/architecture.md`,
  },
  {
    id: 'node-agent',
    label: 'Node Agent',
    eyebrow: 'Outbound control',
    layer: 'node',
    position: [-5, -4.5, 0],
    status: 'verified',
    gate: 'N0 · E0',
    summary:
      'Outbound mTLS long polling applies leased commands and journals exact outcomes across restarts.',
    owns: ['Command journal', 'Artifact transport', 'Observed provider state'],
    boundary: 'The agent has node-local authority only for the exact leased command.',
    docsUrl: `${cloudDocs}/docs/architecture.md`,
  },
  {
    id: 'runtime',
    label: 'A3S Runtime',
    eyebrow: 'Provider-neutral execution',
    layer: 'node',
    position: [0, -4.5, 0],
    status: 'verified',
    gate: 'R0',
    summary:
      'A common Task and Service contract with immutable specs, health, outputs, and idempotent recovery.',
    owns: ['RuntimeUnit', 'Task and Service lifecycle', 'Provider receipt'],
    boundary: 'Runtime contains no Cloud tenant, product asset, or inference scheduling semantics.',
    docsUrl: `${cloudDocs}/docs/architecture.md#3-universal-a3s-runtime-boundary`,
  },
  {
    id: 'gateway',
    label: 'A3S Gateway',
    eyebrow: 'Traffic data plane',
    layer: 'node',
    position: [5, -4.5, 0],
    status: 'verified',
    gate: 'E0',
    summary:
      'Applies complete versioned policy, terminates TLS, streams traffic, and selects healthy targets.',
    owns: ['Transport and TLS', 'Atomic snapshot apply', 'Request-path enforcement'],
    boundary: 'Gateway is not a tenant database, scheduler, rollout controller, or autoscaling authority.',
    docsUrl: 'https://github.com/A3S-Lab/Gateway/blob/main/ROADMAP.md',
  },
  {
    id: 'docker-buildkit',
    label: 'Docker · BuildKit',
    eyebrow: 'Execution providers',
    layer: 'provider',
    position: [-5, -8, 0],
    status: 'in-progress',
    gate: 'R0 · G0',
    summary: 'Docker runs services and isolated BuildKit Tasks produce OCI output under dual network denial.',
    owns: ['Concrete containers', 'Rootless build worker', 'Output capture'],
    boundary: 'Provider identities remain behind Runtime and Artifact ports.',
    docsUrl: `${cloudDocs}/docs/development-plan.md`,
  },
  {
    id: 'workload-unit',
    label: 'Healthy Runtime Unit',
    eyebrow: 'Converged resource',
    layer: 'provider',
    position: [0, -8, 0],
    status: 'verified',
    gate: 'D0 · E0',
    summary: 'A digest-pinned workload revision becomes one observable provider resource and healthy target.',
    owns: ['Provider resource', 'Health evidence', 'Ordered logs'],
    boundary: 'A unit becomes active only after exact health and routing acknowledgement gates pass.',
    docsUrl: `${cloudDocs}/docs/architecture.md`,
  },
  {
    id: 'registry',
    label: 'OCI Registry',
    eyebrow: 'Published artifacts',
    layer: 'provider',
    position: [5, -8, 0],
    status: 'in-progress',
    gate: 'G0',
    summary: 'Authenticated, digest-only publication stores the complete remotely verified OCI graph.',
    owns: ['OCI manifests', 'Configs and layers', 'Digest-addressed release'],
    boundary:
      'Registry credentials are materialized per attempt and never enter build history or provenance.',
    docsUrl: `${cloudDocs}/docs/development-plan.md`,
  },
] as const;

export const ARCHITECTURE_EDGES: readonly ArchitectureEdge[] = [
  {
    id: 'clients-web',
    from: 'clients',
    to: 'web',
    label: 'operator intent',
    journeys: ['deploy', 'observe'],
  },
  {
    id: 'web-api',
    from: 'web',
    to: 'api',
    label: 'commands & queries',
    journeys: ['deploy', 'source', 'observe'],
  },
  {
    id: 'surfaces-api',
    from: 'control-surfaces',
    to: 'api',
    label: 'shared contracts',
    journeys: ['deploy', 'source', 'observe'],
  },
  {
    id: 'github-sources',
    from: 'github',
    to: 'sources',
    label: 'signed event / exact revision',
    journeys: ['source'],
  },
  {
    id: 'api-identity',
    from: 'api',
    to: 'identity',
    label: 'authorize',
    journeys: ['deploy', 'source', 'observe'],
  },
  {
    id: 'api-projects',
    from: 'api',
    to: 'projects',
    label: 'scope',
    journeys: ['deploy', 'source'],
  },
  {
    id: 'api-workloads',
    from: 'api',
    to: 'workloads',
    label: 'desired revision',
    journeys: ['deploy'],
  },
  {
    id: 'api-sources',
    from: 'api',
    to: 'sources',
    label: 'source intent',
    journeys: ['source'],
  },
  {
    id: 'sources-artifacts',
    from: 'sources',
    to: 'artifacts',
    label: 'immutable build input',
    journeys: ['source'],
  },
  {
    id: 'artifacts-operations',
    from: 'artifacts',
    to: 'operations',
    label: 'BuildRun operation',
    journeys: ['source'],
  },
  {
    id: 'workloads-operations',
    from: 'workloads',
    to: 'operations',
    label: 'deployment operation',
    journeys: ['deploy'],
  },
  {
    id: 'operations-flow',
    from: 'operations',
    to: 'flow',
    label: 'durable workflow',
    journeys: ['deploy', 'source'],
  },
  {
    id: 'flow-node-agent',
    from: 'flow',
    to: 'node-agent',
    label: 'leased command',
    journeys: ['deploy', 'source'],
  },
  {
    id: 'node-runtime',
    from: 'node-agent',
    to: 'runtime',
    label: 'apply / remove',
    journeys: ['deploy', 'source'],
  },
  {
    id: 'runtime-provider',
    from: 'runtime',
    to: 'docker-buildkit',
    label: 'provider contract',
    journeys: ['deploy', 'source'],
  },
  {
    id: 'buildkit-registry',
    from: 'docker-buildkit',
    to: 'registry',
    label: 'validated OCI graph',
    journeys: ['source'],
  },
  {
    id: 'registry-workloads',
    from: 'registry',
    to: 'workloads',
    label: 'digest-only handoff',
    journeys: ['source'],
  },
  {
    id: 'runtime-workload',
    from: 'runtime',
    to: 'workload-unit',
    label: 'converged unit',
    journeys: ['deploy'],
  },
  {
    id: 'workload-edge',
    from: 'workload-unit',
    to: 'edge',
    label: 'healthy exact target',
    journeys: ['deploy', 'traffic'],
  },
  {
    id: 'edge-agent',
    from: 'edge',
    to: 'node-agent',
    label: 'complete snapshot',
    journeys: ['deploy'],
  },
  {
    id: 'agent-gateway',
    from: 'node-agent',
    to: 'gateway',
    label: 'apply & acknowledge',
    journeys: ['deploy'],
  },
  {
    id: 'clients-gateway',
    from: 'clients',
    to: 'gateway',
    label: 'HTTPS / streaming',
    journeys: ['traffic'],
  },
  {
    id: 'gateway-workload',
    from: 'gateway',
    to: 'workload-unit',
    label: 'healthy upstream',
    journeys: ['traffic'],
  },
  {
    id: 'workload-agent-logs',
    from: 'workload-unit',
    to: 'node-agent',
    label: 'ordered stdout / stderr',
    journeys: ['observe'],
  },
  {
    id: 'agent-fleet',
    from: 'node-agent',
    to: 'fleet',
    label: 'observation / cursor',
    journeys: ['observe'],
  },
  {
    id: 'fleet-object-store',
    from: 'fleet',
    to: 'object-storage',
    label: 'verified log chunks',
    journeys: ['observe'],
  },
  {
    id: 'object-store-api',
    from: 'object-storage',
    to: 'api',
    label: 'bounded log page',
    journeys: ['observe'],
  },
  {
    id: 'contexts-postgres',
    from: 'workloads',
    to: 'postgres',
    label: 'desired state',
    journeys: ['deploy', 'observe'],
  },
  {
    id: 'artifacts-store',
    from: 'artifacts',
    to: 'object-storage',
    label: 'command-bound archive',
    journeys: ['source'],
  },
  {
    id: 'contexts-event',
    from: 'projects',
    to: 'event',
    label: 'committed fact',
    journeys: ['deploy', 'source'],
  },
  {
    id: 'inference-workloads',
    from: 'inference',
    to: 'workloads',
    label: 'typed execution plan',
    journeys: ['deploy'],
  },
  {
    id: 'inference-fleet',
    from: 'inference',
    to: 'fleet',
    label: 'accelerator claims',
    journeys: ['deploy'],
  },
  {
    id: 'inference-edge',
    from: 'inference',
    to: 'edge',
    label: 'model route policy',
    journeys: ['traffic'],
  },
] as const;

export const JOURNEYS: readonly Journey[] = [
  {
    id: 'all',
    label: 'Complete system',
    shortLabel: 'All',
    description: 'Every current control, build, deployment, traffic, and observation relationship.',
    color: '#b8f36b',
  },
  {
    id: 'deploy',
    label: 'Deploy & converge',
    shortLabel: 'Deploy',
    description: 'From tenant intent to a healthy digest-pinned Runtime unit and acknowledged route.',
    color: '#b8f36b',
  },
  {
    id: 'source',
    label: 'Source to release',
    shortLabel: 'Build',
    description:
      'Exact Git revision, isolated BuildKit Task, verified OCI publication, and Workload handoff.',
    color: '#f3c86b',
  },
  {
    id: 'traffic',
    label: 'Request data plane',
    shortLabel: 'Traffic',
    description: 'Clients reach only Gateway-selected healthy targets; Cloud stays off the request path.',
    color: '#72b7ff',
  },
  {
    id: 'observe',
    label: 'Logs & operations',
    shortLabel: 'Observe',
    description: 'Ordered provider output becomes verified objects, explicit gaps, and resumable UI state.',
    color: '#d7b6ff',
  },
] as const;

export const ARCHITECTURE_GRAPH: ArchitectureGraph = {
  layers: ARCHITECTURE_LAYERS,
  nodes: ARCHITECTURE_NODES,
  edges: ARCHITECTURE_EDGES,
  journeys: JOURNEYS,
};

export function edgesForJourney(
  journey: JourneyId,
  edges: readonly ArchitectureEdge[] = ARCHITECTURE_EDGES
): readonly ArchitectureEdge[] {
  return journey === 'all' ? edges : edges.filter((edge) => edge.journeys.includes(journey));
}

export function nodeIdsForJourney(
  journey: JourneyId,
  edges: readonly ArchitectureEdge[] = ARCHITECTURE_EDGES
): ReadonlySet<string> {
  if (journey === 'all') {
    return new Set(ARCHITECTURE_NODES.map((node) => node.id));
  }
  const nodeIds = new Set<string>();
  for (const edge of edgesForJourney(journey, edges)) {
    nodeIds.add(edge.from);
    nodeIds.add(edge.to);
  }
  return nodeIds;
}

export function validateArchitectureGraph(graph: ArchitectureGraph): readonly string[] {
  const errors: string[] = [];
  const layerIds = new Set(graph.layers.map((layer) => layer.id));
  const nodeIds = new Set<string>();
  const edgeIds = new Set<string>();
  const journeyIds = new Set(graph.journeys.map((journey) => journey.id));

  for (const node of graph.nodes) {
    if (nodeIds.has(node.id)) {
      errors.push(`duplicate node id: ${node.id}`);
    }
    nodeIds.add(node.id);
    if (!layerIds.has(node.layer)) {
      errors.push(`node ${node.id} references missing layer ${node.layer}`);
    }
    if (node.owns.length === 0 || node.summary.trim().length === 0 || node.boundary.trim().length === 0) {
      errors.push(`node ${node.id} is missing explanatory content`);
    }
  }

  for (const edge of graph.edges) {
    if (edgeIds.has(edge.id)) {
      errors.push(`duplicate edge id: ${edge.id}`);
    }
    edgeIds.add(edge.id);
    if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to)) {
      errors.push(`edge ${edge.id} references a missing node`);
    }
    if (edge.from === edge.to) {
      errors.push(`edge ${edge.id} is a self-loop`);
    }
    if (edge.journeys.length === 0 || edge.journeys.some((journey) => !journeyIds.has(journey))) {
      errors.push(`edge ${edge.id} has an invalid journey`);
    }
  }

  for (const journey of graph.journeys) {
    if (journey.id !== 'all' && edgesForJourney(journey.id, graph.edges).length === 0) {
      errors.push(`journey ${journey.id} has no edges`);
    }
  }

  return errors;
}
