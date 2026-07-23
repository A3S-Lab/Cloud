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
    eyebrow: 'Web / Code → Gateway → Cloud',
    description:
      'The public same-origin boundary routes Web and Code API calls to private Cloud services and live requests to healthy workloads.',
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
    size: [30.6, 6.35],
    color: '#b8f36b',
    memberNodeIds: [
      'api',
      'identity',
      'projects',
      'inference',
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
    eyebrow: 'Docker · A3S Box · healthy units',
    description:
      'Conformant provider implementations turn Runtime plans into concrete build tasks and healthy Cloud workload units.',
    position: [0, 0.08, -7.2],
    size: [30.6, 2.05],
    color: '#f3c86b',
    memberNodeIds: ['docker-buildkit', 'box-provider', 'power', 'workload-unit'],
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
    id: 'gateway-routes-web',
    hostNodeIds: ['gateway'],
    guestNodeIds: ['web'],
    label: 'private SPA service',
    description:
      'Gateway routes non-API same-origin requests to the private service that delivers the A3S Web management SPA.',
    hostAction: 'routes non-API paths to',
    guestAction: 'served behind',
    boundary:
      'Gateway does not read SPA files itself, and the Web surface does not gain direct access to private Cloud services.',
    color: '#72b7ff',
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
    id: 'runtime-drives-providers',
    hostNodeIds: ['runtime'],
    guestNodeIds: ['docker-buildkit', 'box-provider'],
    label: 'provider implementations',
    description:
      'A3S Runtime drives interchangeable provider implementations through one normalized lifecycle contract.',
    hostAction: 'drives',
    guestAction: 'implements',
    boundary:
      'Provider-specific APIs and credentials remain behind the Runtime contract instead of leaking into Cloud domains.',
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
      'This verified relationship covers CPU execution only; GPU passthrough for the Box provider remains a planned capability.',
    color: '#d7b6ff',
  },
] as const;
