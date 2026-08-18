export type ArchitectureCarrierId =
  | 'box-local-runtime'
  | 'gateway-public-boundary'
  | 'boot-modular-host'
  | 'durable-service-fabric'
  | 'managed-node-host'
  | 'provider-compute-cluster'
  | 'infrastructure-hardware-cluster';

export interface ArchitectureCarrier {
  id: ArchitectureCarrierId;
  label: string;
  eyebrow: string;
  description: string;
  position: readonly [number, number, number];
  size: readonly [number, number];
  color: string;
  memberNodeIds: readonly string[];
}

export interface ArchitectureHostingRelationship {
  id: string;
  hostNodeIds: readonly string[];
  guestNodeIds: readonly string[];
  label: string;
  description: string;
  hostAction: string;
  guestAction: string;
  boundary: string;
  color: string;
}

export const ARCHITECTURE_CARRIERS: readonly ArchitectureCarrier[] = [
  {
    id: 'box-local-runtime',
    label: 'Local A3S Box Workload Host',
    eyebrow: 'General host · A3S Code is one guest',
    description:
      'A local A3S Box installation carries isolated agent products and tools; A3S Code is one hosted workload.',
    position: [10.675, 0.08, 14],
    size: [6.9, 2.6],
    color: '#71d5c3',
    memberNodeIds: ['a3s-box', 'code-tui'],
  },
  {
    id: 'gateway-public-boundary',
    label: 'A3S Gateway Public Boundary',
    eyebrow: 'Client / Code → Gateway → Cloud',
    description:
      'The public boundary routes TypeScript-client and Code API calls to private Cloud services and live requests to healthy workloads.',
    position: [0, 0.08, 10.3],
    size: [9.8, 1.75],
    color: '#5dd6ff',
    memberNodeIds: ['gateway'],
  },
  {
    id: 'boot-modular-host',
    label: 'A3S Boot Modular Host',
    eyebrow: 'NestJS · DDD module container',
    description:
      'Boot loads the control-plane bounded contexts behind one authenticated application boundary.',
    position: [0, 0.08, 5.2],
    size: [30.6, 8.05],
    color: '#b8f36b',
    memberNodeIds: [
      'api',
      'identity',
      'projects',
      'inference',
      'workflow',
      'agents',
      'evolution',
      'sources',
      'artifacts',
      'workloads',
      'fleet',
      'edge',
      'operations',
    ],
  },
  {
    id: 'durable-service-fabric',
    label: 'Durable Service Fabric',
    eyebrow: 'State · history · facts · bytes',
    description:
      'Stateful middleware carries authoritative truth, replay history, events, and immutable objects.',
    position: [0, 0.08, -0.1],
    size: [30.6, 2.4],
    color: '#d7b6ff',
    memberNodeIds: ['postgres', 'flow', 'event', 'object-storage'],
  },
  {
    id: 'managed-node-host',
    label: 'Managed Node Host',
    eyebrow: 'Outbound control · local execution',
    description: 'One managed node carries its outbound Agent and provider-neutral Runtime.',
    position: [0, 0.08, -3.7],
    size: [12.8, 2.05],
    color: '#71d5c3',
    memberNodeIds: ['node-agent', 'runtime'],
  },
  {
    id: 'provider-compute-cluster',
    label: 'Provider & Workload Runtime Layer',
    eyebrow: 'A3S Box · A3S Power · healthy units',
    description:
      'The sole A3S Box provider turns Runtime plans into isolated builds and healthy Cloud workload units; Power is one planned guest.',
    position: [0, 0.08, -7.2],
    size: [30.6, 2.05],
    color: '#f3c86b',
    memberNodeIds: ['box-provider', 'power', 'workload-unit'],
  },
  {
    id: 'infrastructure-hardware-cluster',
    label: 'Infrastructure & Hardware Cluster',
    eyebrow: 'OCI registry · CPU racks · GPU racks',
    description:
      'Infrastructure distributes immutable OCI content and supplies the physical CPU and GPU capacity consumed by providers.',
    position: [0, 0.08, -10.9],
    size: [30.6, 2.2],
    color: '#ff9f72',
    memberNodeIds: ['registry', 'cpu-compute', 'gpu-compute'],
  },
] as const;

export const ARCHITECTURE_HOSTING_RELATIONSHIPS: readonly ArchitectureHostingRelationship[] = [
  {
    id: 'box-hosts-code',
    hostNodeIds: ['a3s-box'],
    guestNodeIds: ['code-tui'],
    label: 'A3S Code as one local workload',
    description:
      'A local A3S Box supplies the isolated runtime boundary in which the A3S Code product executes.',
    hostAction: 'hosts',
    guestAction: 'runs inside',
    boundary:
      'A3S Code is one optional Box workload; Box remains a general host for other agent products and tools.',
    color: '#71d5c3',
  },
  {
    id: 'boot-hosts-contexts',
    hostNodeIds: ['api'],
    guestNodeIds: [
      'identity',
      'projects',
      'inference',
      'workflow',
      'agents',
      'evolution',
      'sources',
      'artifacts',
      'workloads',
      'fleet',
      'edge',
      'operations',
    ],
    label: 'bounded-context modules',
    description:
      'A3S Boot loads the Cloud business bounded contexts inside one authenticated NestJS application boundary.',
    hostAction: 'loads',
    guestAction: 'loaded by',
    boundary:
      'Each bounded context keeps its own domain and persistence contracts even though Boot composes them in one process.',
    color: '#b8f36b',
  },
  {
    id: 'agent-manages-runtime',
    hostNodeIds: ['node-agent'],
    guestNodeIds: ['runtime'],
    label: 'node-local execution service',
    description:
      'Node Agent supervises the provider-neutral Runtime used to converge declared work on the managed host.',
    hostAction: 'manages',
    guestAction: 'managed by',
    boundary:
      'Agent owns outbound coordination and Runtime owns normalized provider execution; neither replaces the other.',
    color: '#71d5c3',
  },
  {
    id: 'agent-configures-gateway',
    hostNodeIds: ['node-agent'],
    guestNodeIds: ['gateway'],
    label: 'versioned route application',
    description:
      'Node Agent cooperates with Gateway by applying and acknowledging complete versioned routing snapshots.',
    hostAction: 'configures',
    guestAction: 'receives snapshots from',
    boundary:
      'This management relationship does not put Cloud on the live request path or give Gateway scheduling authority.',
    color: '#71d5c3',
  },
  {
    id: 'runtime-drives-box-provider',
    hostNodeIds: ['runtime'],
    guestNodeIds: ['box-provider'],
    label: 'sole provider implementation',
    description:
      'A3S Runtime drives the sole A3S Box provider through one normalized Task, Service, and build lifecycle contract.',
    hostAction: 'drives',
    guestAction: 'implements',
    boundary:
      'Box-specific APIs and credentials stay behind Runtime; Cloud carries no fallback provider or parallel lifecycle state.',
    color: '#f3c86b',
  },
  {
    id: 'box-provider-hosts-workloads',
    hostNodeIds: ['box-provider'],
    guestNodeIds: ['workload-unit'],
    label: 'isolated Cloud workload units',
    description:
      'The A3S Box provider supplies the concrete isolation and lifecycle boundary that carries general Cloud workload units.',
    hostAction: 'hosts',
    guestAction: 'runs inside',
    boundary:
      'The provider is a workload carrier, not a product-specific A3S Code host and not the owner of workload desired state.',
    color: '#71d5c3',
  },
  {
    id: 'workload-runs-power',
    hostNodeIds: ['workload-unit'],
    guestNodeIds: ['power'],
    label: 'optional inference backend workload',
    description:
      'A3S Power executes as one typed backend inside an ordinary Cloud-managed workload unit after its conformance gate passes.',
    hostAction: 'carries',
    guestAction: 'runs inside',
    boundary:
      'Power is not the Workload controller or the only inference backend; the surrounding unit keeps generic lifecycle and health semantics.',
    color: '#b69cff',
  },
  {
    id: 'cpu-supplies-box-provider',
    hostNodeIds: ['cpu-compute'],
    guestNodeIds: ['box-provider'],
    label: 'Box provider execution',
    description:
      'CPU rack capacity supplies the host compute on which the current A3S Box provider implementation executes.',
    hostAction: 'supplies compute to',
    guestAction: 'runs on',
    boundary:
      'CPU execution is available while the complete Box-only baseline still awaits BX0 re-certification.',
    color: '#d7b6ff',
  },
  {
    id: 'gpu-supplies-box-provider',
    hostNodeIds: ['gpu-compute'],
    guestNodeIds: ['box-provider'],
    label: 'planned Box GPU binding',
    description:
      'GPU rack capacity will be exposed through the same A3S Box provider used for every other Runtime unit.',
    hostAction: 'supplies accelerator capacity to',
    guestAction: 'binds through',
    boundary:
      'No direct Power-to-device lifecycle exists; Box GPU passthrough remains planned until BX0 and PW0 evidence passes.',
    color: '#d7b6ff',
  },
] as const;
