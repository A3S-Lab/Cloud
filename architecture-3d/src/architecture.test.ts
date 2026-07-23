import { describe, expect, it } from 'vitest';
import {
  ARCHITECTURE_DOMAINS,
  ARCHITECTURE_EDGES,
  ARCHITECTURE_GRAPH,
  ARCHITECTURE_LOGO_IDS,
  ARCHITECTURE_NODES,
  ARCHITECTURE_VISUAL_KINDS,
  JOURNEYS,
  edgesForJourney,
  nodeIdsForJourney,
} from './architecture';
import { validateArchitectureGraph } from './architecture-validation';
import { SIMULATION_ENTRIES, SIMULATION_SCENARIOS, simulationFramesFor } from './simulations';
import { ARCHITECTURE_CARRIERS, ARCHITECTURE_HOSTING_RELATIONSHIPS } from './topology';

describe('architecture graph', () => {
  it('has closed node, domain, visual, edge, and journey references', () => {
    expect(validateArchitectureGraph(ARCHITECTURE_GRAPH)).toEqual([]);
  });

  it('places every node inside a bird-eye domain district with an explicit facility and logo', () => {
    const visualKinds = new Set<string>(ARCHITECTURE_VISUAL_KINDS);
    const logoIds = new Set<string>(ARCHITECTURE_LOGO_IDS);

    for (const node of ARCHITECTURE_NODES) {
      const domain = ARCHITECTURE_DOMAINS.find((candidate) => candidate.id === node.domain);
      expect(domain, `${node.id} domain`).toBeDefined();
      if (!domain) continue;
      expect(Math.abs(node.position[0] - domain.center[0])).toBeLessThan(domain.size[0] / 2);
      expect(Math.abs(node.position[2] - domain.center[1])).toBeLessThan(domain.size[1] / 2);
      expect(visualKinds.has(node.visualKind), `${node.id} visual`).toBe(true);
      expect(logoIds.has(node.logoId), `${node.id} logo`).toBe(true);
    }

    expect(ARCHITECTURE_NODES.some((node) => node.id === 'a3s-box')).toBe(true);
    expect(ARCHITECTURE_NODES.some((node) => node.id === 'box-provider')).toBe(true);
    expect(ARCHITECTURE_NODES.some((node) => node.id === 'code-tui')).toBe(true);
    expect(ARCHITECTURE_NODES.some((node) => node.id === 'cpu-compute')).toBe(true);
    expect(ARCHITECTURE_NODES.some((node) => node.id === 'gpu-compute')).toBe(true);
  });

  it('models A3S Box as a general carrier with A3S Code as one local workload', () => {
    const boxCarrier = ARCHITECTURE_CARRIERS.find((carrier) => carrier.id === 'box-local-runtime');
    const codeHosting = ARCHITECTURE_HOSTING_RELATIONSHIPS.find(
      (relationship) => relationship.id === 'box-hosts-code'
    );
    const cloudHosting = ARCHITECTURE_HOSTING_RELATIONSHIPS.find(
      (relationship) => relationship.id === 'box-provider-hosts-workloads'
    );
    const providerCarrier = ARCHITECTURE_CARRIERS.find(
      (carrier) => carrier.id === 'provider-compute-cluster'
    );

    expect(boxCarrier?.memberNodeIds).toEqual(['a3s-box', 'code-tui']);
    expect(codeHosting?.hostNodeIds).toEqual(['a3s-box']);
    expect(codeHosting?.guestNodeIds).toEqual(['code-tui']);
    expect(cloudHosting?.hostNodeIds).toEqual(['box-provider']);
    expect(cloudHosting?.guestNodeIds).toEqual(['workload-unit']);
    expect(providerCarrier?.memberNodeIds).toContain('box-provider');
    expect(ARCHITECTURE_EDGES.some((edge) => edge.id === 'box-hosts-code')).toBe(false);
    expect(ARCHITECTURE_EDGES.some((edge) => edge.id === 'runtime-box')).toBe(true);
    expect(ARCHITECTURE_EDGES.some((edge) => edge.id === 'box-workload')).toBe(true);
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

  it('keeps every simulation frame closed over real modules and business relationships', () => {
    const nodeIds = new Set(ARCHITECTURE_NODES.map((node) => node.id));
    const edgeIds = new Set(ARCHITECTURE_EDGES.map((edge) => edge.id));

    for (const entry of SIMULATION_ENTRIES) {
      for (const scenario of SIMULATION_SCENARIOS) {
        const frames = simulationFramesFor(entry.id, scenario.id);
        expect(frames.length).toBeGreaterThan(4);
        for (const frame of frames) {
          expect(frame.nodeIds.length).toBeGreaterThan(1);
          expect(
            frame.nodeIds.every((nodeId) => nodeIds.has(nodeId)),
            `${scenario.id}/${frame.id} nodes`
          ).toBe(true);
          expect(
            frame.edgeIds.every((edgeId) => edgeIds.has(edgeId)),
            `${scenario.id}/${frame.id} edges`
          ).toBe(true);
        }
      }
    }

    const cpuExecution = SIMULATION_SCENARIOS.find((scenario) => scenario.id === 'deploy-cpu')?.steps.find(
      (step) => step.id === 'execute-cpu'
    );
    const gpuExecution = SIMULATION_SCENARIOS.find((scenario) => scenario.id === 'gpu-inference')?.steps.find(
      (step) => step.id === 'execute-gpu'
    );
    expect(cpuExecution?.nodeIds).toContain('box-provider');
    expect(cpuExecution?.edgeIds).toContain('box-workload');
    expect(gpuExecution?.nodeIds).not.toContain('box-provider');
  });
});
