export type ArchitectureCarrierId =
  | 'box-local-runtime'
  | 'boot-modular-host'
  | 'durable-service-fabric'
  | 'managed-node-host'
  | 'provider-compute-cluster';

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
  hostAction: string;
  guestAction: string;
  color: string;
}

export const ARCHITECTURE_CARRIERS: readonly ArchitectureCarrier[] = [
  {
    id: 'box-local-runtime',
    label: 'Local A3S Box Workload Host',
    eyebrow: 'General host · A3S Code is one guest',
    description:
      'A local A3S Box installation carries isolated agent products and tools; A3S Code is one hosted workload.',
    position: [10.675, 0.08, 9],
    size: [6.9, 3.8],
    color: '#71d5c3',
    memberNodeIds: ['a3s-box', 'code-tui'],
  },
  {
    id: 'boot-modular-host',
    label: 'A3S Boot Modular Host',
    eyebrow: 'NestJS · DDD module container',
    description:
      'Boot loads the control-plane bounded contexts behind one authenticated application boundary.',
    position: [0, 0.08, 2.1],
    size: [30.6, 7.15],
    color: '#b8f36b',
    memberNodeIds: [
      'api',
      'identity',
      'projects',
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
    position: [-10.7, 0.08, -8.1],
    size: [9.15, 7.15],
    color: '#d7b6ff',
    memberNodeIds: ['postgres', 'flow', 'event', 'object-storage'],
  },
  {
    id: 'managed-node-host',
    label: 'Managed Node Host',
    eyebrow: 'Outbound control · local execution',
    description: 'One managed node carries its Agent, provider-neutral Runtime, and Gateway data plane.',
    position: [0, 0.08, -8.1],
    size: [8.4, 7.15],
    color: '#71d5c3',
    memberNodeIds: ['node-agent', 'runtime', 'gateway'],
  },
  {
    id: 'provider-compute-cluster',
    label: 'Provider Compute Cluster',
    eyebrow: 'Docker · A3S Box · CPU/GPU racks',
    description:
      'Conformant providers and hardware racks carry concrete build tasks and healthy Cloud workload units.',
    position: [10.7, 0.08, -8.1],
    size: [9.15, 7.15],
    color: '#f3c86b',
    memberNodeIds: ['docker-buildkit', 'box-provider', 'workload-unit', 'cpu-compute', 'gpu-compute'],
  },
] as const;

export const ARCHITECTURE_HOSTING_RELATIONSHIPS: readonly ArchitectureHostingRelationship[] = [
  {
    id: 'box-hosts-code',
    hostNodeIds: ['a3s-box'],
    guestNodeIds: ['code-tui'],
    label: 'A3S Code as one local workload',
    hostAction: 'hosts',
    guestAction: 'runs inside',
    color: '#71d5c3',
  },
  {
    id: 'boot-hosts-contexts',
    hostNodeIds: ['api'],
    guestNodeIds: [
      'identity',
      'projects',
      'sources',
      'artifacts',
      'workloads',
      'fleet',
      'edge',
      'operations',
    ],
    label: 'bounded-context modules',
    hostAction: 'loads',
    guestAction: 'loaded by',
    color: '#b8f36b',
  },
  {
    id: 'gateway-hosts-web',
    hostNodeIds: ['gateway'],
    guestNodeIds: ['web'],
    label: 'same-origin Web SPA',
    hostAction: 'serves',
    guestAction: 'served by',
    color: '#72b7ff',
  },
  {
    id: 'agent-hosts-node-services',
    hostNodeIds: ['node-agent'],
    guestNodeIds: ['runtime', 'gateway'],
    label: 'node-local services',
    hostAction: 'manages',
    guestAction: 'managed by',
    color: '#71d5c3',
  },
  {
    id: 'runtime-drives-providers',
    hostNodeIds: ['runtime'],
    guestNodeIds: ['docker-buildkit', 'box-provider'],
    label: 'provider implementations',
    hostAction: 'drives',
    guestAction: 'implements',
    color: '#f3c86b',
  },
  {
    id: 'box-provider-hosts-workloads',
    hostNodeIds: ['box-provider'],
    guestNodeIds: ['workload-unit'],
    label: 'isolated Cloud workload units',
    hostAction: 'hosts',
    guestAction: 'runs inside',
    color: '#71d5c3',
  },
  {
    id: 'cpu-supplies-box-provider',
    hostNodeIds: ['cpu-compute'],
    guestNodeIds: ['box-provider'],
    label: 'Box provider execution',
    hostAction: 'supplies compute to',
    guestAction: 'runs on',
    color: '#d7b6ff',
  },
] as const;
