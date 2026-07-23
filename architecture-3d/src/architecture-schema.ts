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

export const ARCHITECTURE_DOMAIN_IDS = [
  'experience',
  'control',
  'coordination',
  'data-plane',
  'ecosystem',
] as const;

export type ArchitectureDomainId = (typeof ARCHITECTURE_DOMAIN_IDS)[number];

export const ARCHITECTURE_VISUAL_KINDS = [
  'client-terminal',
  'web-console',
  'box-runtime',
  'box-workload-host',
  'code-terminal',
  'source-repository',
  'gpu-cluster',
  'control-tower',
  'identity-vault',
  'project-blocks',
  'source-branch',
  'artifact-factory',
  'workload-cluster',
  'fleet-radar',
  'edge-router',
  'operations-timeline',
  'database',
  'workflow-orchestrator',
  'event-bus',
  'object-storage',
  'node-antenna',
  'runtime-engine',
  'traffic-gateway',
  'buildkit-yard',
  'healthy-runtime',
  'registry-rack',
  'cpu-array',
  'gpu-array',
] as const;

export type ArchitectureVisualKind = (typeof ARCHITECTURE_VISUAL_KINDS)[number];

export const ARCHITECTURE_LOGO_IDS = [
  'clients',
  'a3s-web',
  'a3s-box',
  'a3s-box-provider',
  'a3s-code',
  'github',
  'inference',
  'a3s-boot',
  'identity',
  'projects',
  'sources',
  'artifacts',
  'workloads',
  'fleet',
  'edge',
  'operations',
  'postgresql',
  'a3s-flow',
  'a3s-event',
  'object-store',
  'node-agent',
  'a3s-runtime',
  'a3s-gateway',
  'docker-buildkit',
  'runtime-unit',
  'oci-registry',
  'cpu-compute',
  'gpu-compute',
] as const;

export type ArchitectureLogoId = (typeof ARCHITECTURE_LOGO_IDS)[number];

export type JourneyId = 'all' | 'deploy' | 'source' | 'traffic' | 'observe';

export interface ArchitectureDomain {
  id: ArchitectureDomainId;
  label: string;
  shortLabel: string;
  description: string;
  center: readonly [number, number];
  size: readonly [number, number];
  color: string;
}

export interface ArchitectureNode {
  id: string;
  label: string;
  eyebrow: string;
  domain: ArchitectureDomainId;
  position: readonly [number, number, number];
  visualKind: ArchitectureVisualKind;
  logoId: ArchitectureLogoId;
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
  domains: readonly ArchitectureDomain[];
  nodes: readonly ArchitectureNode[];
  edges: readonly ArchitectureEdge[];
  journeys: readonly Journey[];
}
