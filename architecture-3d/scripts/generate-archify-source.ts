import {
  ARCHITECTURE_GRAPH,
  ARCHITECTURE_STATUS_META,
  type ArchitectureEdge,
  type ArchitectureNode,
} from '../src/architecture';
import { writeFile } from 'node:fs/promises';
import { archifyStructuralConnectionId } from '../src/archify-bridge';
import { ARCHITECTURE_HOSTING_RELATIONSHIPS } from '../src/topology';

type ArchifyComponentType =
  | 'frontend'
  | 'backend'
  | 'database'
  | 'cloud'
  | 'security'
  | 'messagebus'
  | 'external';
type ArchifyConnectionVariant = 'default' | 'emphasis' | 'security' | 'dashed';

const document = {
  schema_version: 1,
  diagram_type: 'architecture',
  meta: {
    title: 'A3S Cloud Layered Architecture',
    subtitle:
      'Experience → Unified Gateway → Cloud product semantics → one durable control path → providers → infrastructure',
    output: 'public/archify/a3s-cloud.architecture.html',
    animation: 'trace',
    visual_preset: 'blueprint',
    quality_profile: 'standard',
    viewBox: [1760, 1120],
    views: [
      {
        id: 'management-boundary',
        label: 'Management boundary',
        focus: ['clients', 'cloud-client', 'a3s-box', 'code-tui', 'gateway', 'api'],
        note: 'The maintained client and Code both cross the public Gateway before the private Boot API.',
      },
      {
        id: 'product-semantics',
        label: 'Workflow, Agents, and Evolution',
        focus: [
          'api',
          'workflow',
          'agents',
          'evolution',
          'operations',
          'postgres',
          'flow',
          'workloads',
        ],
        note: 'New product semantics retain PostgreSQL authority and reuse Operations, Flow, and Workloads.',
      },
      {
        id: 'deploy-converge',
        label: 'Deploy and converge',
        focus: [
          'api',
          'workloads',
          'operations',
          'flow',
          'node-agent',
          'runtime',
          'box-provider',
          'workload-unit',
        ],
        note: 'Follow desired state through durable coordination into one provider-backed unit.',
      },
      {
        id: 'power-gpu',
        label: 'Cloud Inference and A3S Power',
        focus: ['inference', 'workloads', 'fleet', 'runtime', 'power', 'gpu-compute'],
        note: 'Cloud Inference owns intent; Power is one backend inside a managed workload.',
      },
      {
        id: 'live-request',
        label: 'Live request path',
        focus: ['clients', 'gateway', 'workload-unit', 'cpu-compute', 'gpu-compute'],
        note: 'Gateway reaches an acknowledged target while Cloud control services stay off-path.',
      },
    ],
  },
  components: ARCHITECTURE_GRAPH.nodes.map((node) => ({
    id: node.id,
    type: componentType(node),
    label: node.label,
    sublabel: node.eyebrow,
    tag: `${node.gate} · ${ARCHITECTURE_STATUS_META[node.status].label}`,
    pos: componentPosition(node),
    size: componentSize(node),
  })),
  boundaries: ARCHITECTURE_GRAPH.domains.map((domain) => ({
    kind: domain.id === 'access' ? 'security-group' : 'region',
    label: `${domain.shortLabel} · ${domain.label}`,
    wraps: ARCHITECTURE_GRAPH.nodes
      .filter((node) => node.domain === domain.id)
      .map((node) => node.id),
    pad: domain.id === 'access' ? 18 : 22,
  })),
  connections: [
    ...ARCHITECTURE_GRAPH.edges.map((edge) => ({
      id: edge.id,
      from: edge.from,
      to: edge.to,
      variant: connectionVariant(edge),
      ...connectionRoute(edge.id),
    })),
    ...ARCHITECTURE_HOSTING_RELATIONSHIPS.flatMap((relationship) =>
      relationship.hostNodeIds.flatMap((hostNodeId) =>
        relationship.guestNodeIds
          .filter((guestNodeId) => guestNodeId !== hostNodeId)
          .map((guestNodeId) => ({
            id: archifyStructuralConnectionId(
              relationship.id,
              hostNodeId,
              guestNodeId
            ),
            from: hostNodeId,
            to: guestNodeId,
            variant: 'dashed' as const,
            ...connectionRoute(
              archifyStructuralConnectionId(relationship.id, hostNodeId, guestNodeId)
            ),
          }))
      )
    ),
  ],
  cards: [
    {
      dot: 'violet',
      title: 'One mechanism for each concern',
      items: [
        'PostgreSQL owns business truth; Operations plus Flow own durable orchestration.',
        'Outbox plus Event publish facts; Fleet alone retains node commands and receipts.',
      ],
    },
    {
      dot: 'orange',
      title: 'Providers do not become infrastructure authority',
      items: [
        'Runtime drives conformant providers through typed contracts.',
        'Registry plus CPU/GPU racks remain a lower infrastructure and hardware layer.',
      ],
    },
    {
      dot: 'emerald',
      title: 'Additive product projection',
      items: [
        'Workflow, heterogeneous Agents, and Evolution add semantics, not new engines.',
        'Existing tenancy, delivery, execution, edge, security, storage, and recovery capabilities remain.',
      ],
    },
  ],
} as const;

const outputUrl = new URL('../archify/a3s-cloud.architecture.json', import.meta.url);
await writeFile(outputUrl, `${JSON.stringify(document, null, 2)}\n`, 'utf8');
process.stdout.write(`Generated ${outputUrl.pathname}\n`);

function componentType(node: ArchitectureNode): ArchifyComponentType {
  if (node.status === 'external' || node.id === 'github') return 'external';
  if (node.id === 'cloud-client' || node.id === 'code-tui') return 'frontend';
  if (node.id === 'gateway' || node.id === 'identity') return 'security';
  if (node.id === 'postgres' || node.id === 'object-storage' || node.id === 'registry') {
    return 'database';
  }
  if (node.id === 'event') return 'messagebus';
  if (
    node.id === 'a3s-box' ||
    node.id === 'workload-unit' ||
    node.id === 'cpu-compute' ||
    node.id === 'gpu-compute'
  ) {
    return 'cloud';
  }
  return 'backend';
}

function componentPosition(node: ArchitectureNode): readonly [number, number] {
  const [x, , z] = node.position;
  const yOffset = node.id === 'inference' ? 12 : 0;
  return [
    Math.round(58 + (x + 14.5) * 54),
    Math.round(72 + (14 - z) * 35.5 + yOffset),
  ];
}

function componentSize(node: ArchitectureNode): readonly [number, number] {
  return node.label.length > 20 ? [148, 64] : [132, 62];
}

function connectionVariant(edge: ArchitectureEdge): ArchifyConnectionVariant {
  if (edge.id === 'cloud-client-gateway' || edge.id === 'code-gateway' || edge.id === 'gateway-api') {
    return 'emphasis';
  }
  if (edge.id.includes('identity') || edge.label.toLowerCase().includes('authorize')) {
    return 'security';
  }
  if (edge.journeys.includes('traffic')) return 'emphasis';
  if (edge.journeys.includes('observe') || edge.id.includes('event')) return 'dashed';
  return 'default';
}

function connectionRoute(id: string): Readonly<Record<string, unknown>> {
  const routes: Readonly<Record<string, Readonly<Record<string, unknown>>>> = {
    'api-evolution': {
      fromSide: 'right',
      toSide: 'right',
      via: [
        [1050, 360],
        [1600, 360],
        [1600, 246],
      ],
    },
    'workflow-operations': {
      fromSide: 'left',
      toSide: 'bottom',
      via: [
        [60, 246],
        [60, 638],
        [1327, 638],
      ],
    },
    'workflow-agents': {
      fromSide: 'top',
      toSide: 'top',
      via: [
        [301, 140],
        [1111, 140],
      ],
    },
    'agents-workloads': {
      fromSide: 'bottom',
      toSide: 'bottom',
      via: [
        [1111, 360],
        [1680, 360],
        [1680, 638],
        [355, 638],
      ],
    },
    'inference-power': {
      fromSide: 'bottom',
      toSide: 'top',
      via: [
        [907, 470],
        [907, 792],
        [1123, 792],
      ],
    },
    'workloads-operations': {
      fromSide: 'bottom',
      toSide: 'bottom',
      via: [
        [487, 540],
        [1327, 540],
      ],
    },
    'box-workload': {
      fromSide: 'top',
      toSide: 'top',
      via: [
        [757, 792],
        [1555, 792],
      ],
    },
    'gateway-workload': {
      fromSide: 'right',
      toSide: 'right',
      via: [
        [1680, 234],
        [1680, 856],
      ],
    },
    'agent-fleet': {
      fromSide: 'right',
      toSide: 'bottom',
      via: [
        [850, 731],
        [850, 540],
        [745, 540],
      ],
    },
    'object-store-api': {
      fromSide: 'right',
      toSide: 'right',
      via: [
        [1680, 604],
        [1680, 330],
      ],
    },
    'fleet-gpu': {
      fromSide: 'bottom',
      toSide: 'right',
      via: [
        [745, 540],
        [1680, 540],
        [1680, 1004],
      ],
    },
    'struct-box-provider-hosts-workloads-box-provider-workload-unit': {
      fromSide: 'top',
      toSide: 'top',
      via: [
        [757, 796],
        [1555, 796],
      ],
    },
    'struct-boot-hosts-contexts-api-evolution': {
      fromSide: 'right',
      toSide: 'right',
      via: [
        [1046, 364],
        [1604, 364],
        [1604, 246],
      ],
    },
  };
  return routes[id] ?? {};
}
