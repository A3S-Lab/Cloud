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

  it('places Gateway between management clients and the private Cloud API', () => {
    const web = ARCHITECTURE_NODES.find((node) => node.id === 'web');
    const code = ARCHITECTURE_NODES.find((node) => node.id === 'code-tui');
    const gateway = ARCHITECTURE_NODES.find((node) => node.id === 'gateway');
    const api = ARCHITECTURE_NODES.find((node) => node.id === 'api');
    const edgeIds = new Set(ARCHITECTURE_EDGES.map((edge) => edge.id));

    expect(gateway?.domain).toBe('access');
    expect(web?.position[2]).toBeGreaterThan(gateway?.position[2] ?? Number.POSITIVE_INFINITY);
    expect(code?.position[2]).toBeGreaterThan(gateway?.position[2] ?? Number.POSITIVE_INFINITY);
    expect(gateway?.position[2]).toBeGreaterThan(api?.position[2] ?? Number.POSITIVE_INFINITY);
    expect(edgeIds.has('web-gateway')).toBe(true);
    expect(edgeIds.has('code-gateway')).toBe(true);
    expect(edgeIds.has('gateway-api')).toBe(true);
    expect(edgeIds.has('web-api')).toBe(false);
    expect(edgeIds.has('code-api')).toBe(false);
  });

  it('separates middleware, node runtime, providers, and physical infrastructure into layers', () => {
    const domainOrder = ['control', 'coordination', 'data-plane', 'ecosystem', 'infrastructure'];
    const centers = domainOrder.map(
      (domainId) => ARCHITECTURE_DOMAINS.find((domain) => domain.id === domainId)?.center[1]
    );
    for (let index = 1; index < centers.length; index += 1) {
      expect(centers[index - 1]).toBeGreaterThan(centers[index] ?? Number.POSITIVE_INFINITY);
    }

    expect(ARCHITECTURE_NODES.find((node) => node.id === 'postgres')?.domain).toBe('coordination');
    expect(ARCHITECTURE_NODES.find((node) => node.id === 'runtime')?.domain).toBe('data-plane');
    expect(ARCHITECTURE_NODES.find((node) => node.id === 'box-provider')?.domain).toBe('ecosystem');
    expect(ARCHITECTURE_NODES.find((node) => node.id === 'gpu-compute')?.domain).toBe('infrastructure');
  });

  it('distinguishes the Cloud Inference context from the A3S Power backend', () => {
    const inference = ARCHITECTURE_NODES.find((node) => node.id === 'inference');
    const power = ARCHITECTURE_NODES.find((node) => node.id === 'power');
    const powerHosting = ARCHITECTURE_HOSTING_RELATIONSHIPS.find(
      (relationship) => relationship.id === 'workload-runs-power'
    );

    expect(inference?.label).toBe('Cloud Inference (I0)');
    expect(inference?.domain).toBe('control');
    expect(power?.label).toBe('A3S Power');
    expect(power?.domain).toBe('ecosystem');
    expect(ARCHITECTURE_EDGES.some((edge) => edge.id === 'inference-power')).toBe(true);
    expect(ARCHITECTURE_EDGES.some((edge) => edge.id === 'power-gpu')).toBe(true);
    expect(powerHosting?.hostNodeIds).toEqual(['workload-unit']);
    expect(powerHosting?.guestNodeIds).toEqual(['power']);
  });

  it('provides detailed HUD content for every business and structural relationship', () => {
    for (const edge of ARCHITECTURE_EDGES) {
      expect(edge.summary.trim(), edge.id).not.toBe('');
      expect(edge.transfers.length, edge.id).toBeGreaterThan(0);
      expect(edge.boundary.trim(), edge.id).not.toBe('');
    }
    for (const relationship of ARCHITECTURE_HOSTING_RELATIONSHIPS) {
      expect(relationship.description.trim(), relationship.id).not.toBe('');
      expect(relationship.boundary.trim(), relationship.id).not.toBe('');
    }
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
