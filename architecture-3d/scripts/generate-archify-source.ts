import {
  ARCHITECTURE_GRAPH,
  ARCHITECTURE_STATUS_META,
  type ArchitectureEdge,
  type ArchitectureNode,
} from '../src/architecture';
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
      'Experience → Gateway → Cloud domains → middleware → managed nodes → providers → infrastructure',
    output: 'public/archify/a3s-cloud.architecture.html',
    animation: 'trace',
    visual_preset: 'blueprint',
    quality_profile: 'standard',
    viewBox: [1760, 1120],
    views: [
      {
        id: 'management-boundary',
        label: 'Management boundary',
        focus: ['clients', 'web', 'a3s-box', 'code-tui', 'gateway', 'api'],
        note: 'Web and Code both cross the public Gateway before the private Boot API.',
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
        id: 'middleware-layer',
        label: 'Middleware and durable truth',
        focus: ['postgres', 'flow', 'event', 'object-storage'],
        note: 'SQL truth, workflow history, committed facts, and immutable bytes stay distinct.',
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
      title: 'Control and middleware stay separate',
      items: [
        'Cloud bounded contexts own business intent and policy.',
        'PostgreSQL, Flow, Event, and object storage provide distinct durable capabilities.',
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
      title: 'Inference ownership',
      items: [
        'Cloud Inference owns models, backend profiles, routes, and usage.',
        'A3S Power is one optional backend carried by a generic Cloud workload.',
      ],
    },
  ],
} as const;

const outputUrl = new URL('../archify/a3s-cloud.architecture.json', import.meta.url);
await Bun.write(outputUrl, `${JSON.stringify(document, null, 2)}\n`);
process.stdout.write(`Generated ${outputUrl.pathname}\n`);

function componentType(node: ArchitectureNode): ArchifyComponentType {
  if (node.status === 'external' || node.id === 'github') return 'external';
  if (node.id === 'web' || node.id === 'code-tui') return 'frontend';
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
  if (edge.id === 'web-gateway' || edge.id === 'code-gateway' || edge.id === 'gateway-api') {
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
  };
  return routes[id] ?? {};
}
