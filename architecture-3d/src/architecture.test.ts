import { describe, expect, it } from 'vitest';
import {
  ARCHITECTURE_EDGES,
  ARCHITECTURE_GRAPH,
  ARCHITECTURE_NODES,
  JOURNEYS,
  edgesForJourney,
  nodeIdsForJourney,
  validateArchitectureGraph,
} from './architecture';

describe('architecture graph', () => {
  it('has closed node, layer, edge, and journey references', () => {
    expect(validateArchitectureGraph(ARCHITECTURE_GRAPH)).toEqual([]);
  });

  it('keeps every named journey connected to at least one end-to-end system boundary', () => {
    for (const journey of JOURNEYS) {
      if (journey.id === 'all') continue;
      const edges = edgesForJourney(journey.id);
      const nodeIds = nodeIdsForJourney(journey.id);

      expect(edges.length).toBeGreaterThan(2);
      expect(nodeIds.size).toBeGreaterThan(3);
      expect(edges.every((edge) => edge.journeys.some((edgeJourney) => edgeJourney === journey.id))).toBe(
        true
      );
    }
  });

  it('makes the complete-system view include every edge and node', () => {
    expect(edgesForJourney('all')).toEqual(ARCHITECTURE_EDGES);
    expect(nodeIdsForJourney('all')).toEqual(new Set(ARCHITECTURE_NODES.map((node) => node.id)));
  });

  it('keeps Cloud off the live request path', () => {
    const trafficEdges = edgesForJourney('traffic');
    const controlPlaneNodes = new Set([
      'api',
      'identity',
      'projects',
      'sources',
      'artifacts',
      'workloads',
      'fleet',
      'operations',
      'postgres',
      'flow',
      'event',
      'object-storage',
    ]);

    expect(
      trafficEdges.filter((edge) => edge.from === 'clients').every((edge) => edge.to === 'gateway')
    ).toBe(true);
    expect(trafficEdges.some((edge) => controlPlaneNodes.has(edge.from) && edge.to === 'workload-unit')).toBe(
      false
    );
  });
});
