import {
  ARCHITECTURE_LOGO_IDS,
  ARCHITECTURE_VISUAL_KINDS,
  type ArchitectureGraph,
  edgesForJourney,
} from './architecture';
import { ARCHITECTURE_CARRIERS, ARCHITECTURE_HOSTING_RELATIONSHIPS } from './topology';

export function validateArchitectureGraph(graph: ArchitectureGraph): readonly string[] {
  const errors: string[] = [];
  const domainIds = new Set(graph.domains.map((domain) => domain.id));
  const visualKinds = new Set<string>(ARCHITECTURE_VISUAL_KINDS);
  const logoIds = new Set<string>(ARCHITECTURE_LOGO_IDS);
  const nodeIds = new Set<string>();
  const edgeIds = new Set<string>();
  const journeyIds = new Set(graph.journeys.map((journey) => journey.id));

  for (const node of graph.nodes) {
    if (nodeIds.has(node.id)) {
      errors.push(`duplicate node id: ${node.id}`);
    }
    nodeIds.add(node.id);
    if (!domainIds.has(node.domain)) {
      errors.push(`node ${node.id} references missing domain ${node.domain}`);
    }
    if (!visualKinds.has(node.visualKind) || !logoIds.has(node.logoId)) {
      errors.push(`node ${node.id} is missing a valid facility visual or logo`);
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

  const carrierIds = new Set<string>();
  for (const carrier of ARCHITECTURE_CARRIERS) {
    if (carrierIds.has(carrier.id)) errors.push(`duplicate carrier id: ${carrier.id}`);
    carrierIds.add(carrier.id);
    for (const memberNodeId of carrier.memberNodeIds) {
      const node = graph.nodes.find((candidate) => candidate.id === memberNodeId);
      if (!node) {
        errors.push(`carrier ${carrier.id} references missing node ${memberNodeId}`);
        continue;
      }
      if (
        Math.abs(node.position[0] - carrier.position[0]) >= carrier.size[0] / 2 ||
        Math.abs(node.position[2] - carrier.position[2]) >= carrier.size[1] / 2
      ) {
        errors.push(`carrier ${carrier.id} does not contain member ${memberNodeId}`);
      }
    }
  }

  const hostingRelationshipIds = new Set<string>();
  for (const relationship of ARCHITECTURE_HOSTING_RELATIONSHIPS) {
    if (hostingRelationshipIds.has(relationship.id)) {
      errors.push(`duplicate hosting relationship id: ${relationship.id}`);
    }
    hostingRelationshipIds.add(relationship.id);
    if ([...relationship.hostNodeIds, ...relationship.guestNodeIds].some((nodeId) => !nodeIds.has(nodeId))) {
      errors.push(`hosting relationship ${relationship.id} references a missing node`);
    }
    if (relationship.hostNodeIds.some((nodeId) => relationship.guestNodeIds.includes(nodeId))) {
      errors.push(`hosting relationship ${relationship.id} hosts itself`);
    }
    if (relationship.hostAction.trim().length === 0 || relationship.guestAction.trim().length === 0) {
      errors.push(`hosting relationship ${relationship.id} is missing directional actions`);
    }
  }

  return errors;
}
