import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { type ArchitectureGraph, type JourneyId, nodeIdsForJourney } from '../architecture';
import { ARCHITECTURE_CARRIERS, ARCHITECTURE_HOSTING_RELATIONSHIPS } from '../topology';
import {
  createArchitectureCarrierVisual,
  disposeArchitectureCarrierVisual,
  updateArchitectureCarrierVisual,
} from './carriers';
import {
  createArchitectureEdgeVisual,
  disposeArchitectureEdgeVisual,
  setArchitectureEdgeJourney,
  updateArchitectureEdgeVisual,
} from './edges';
import {
  createArchitectureEnvironment,
  disposeArchitectureEnvironment,
  updateArchitectureEnvironment,
} from './environment';
import {
  createArchitectureHostingVisual,
  disposeArchitectureHostingVisual,
  updateArchitectureHostingVisual,
} from './hosting';
import { type ArchitectureHoverEvent, attachArchitectureInteraction } from './interaction';
import {
  createArchitectureNodeVisual,
  disposeArchitectureNodeVisual,
  updateArchitectureNodeVisual,
} from './nodes';

interface ArchitectureRuntimeOptions {
  graph: ArchitectureGraph;
  initialJourney: JourneyId;
  initialSelectedNodeId?: string;
  autoRotate: boolean;
  onHover: (event?: ArchitectureHoverEvent) => void;
  onSelect: (nodeId: string) => void;
}

export interface ArchitectureRuntime {
  focusNode: (nodeId: string) => void;
  resetCamera: () => void;
  setAutoRotate: (enabled: boolean) => void;
  setJourney: (journey: JourneyId) => void;
  setSelectedNode: (nodeId?: string) => void;
  setSimulationFrame: (frame?: ArchitectureSimulationFrame) => void;
  dispose: () => void;
}

export interface ArchitectureSimulationFrame {
  nodeIds: readonly string[];
  edgeIds: readonly string[];
}

interface CameraFlight {
  startedAt: number;
  duration: number;
  fromCamera: THREE.Vector3;
  fromTarget: THREE.Vector3;
  toCamera: THREE.Vector3;
  toTarget: THREE.Vector3;
}

const INITIAL_CAMERA = new THREE.Vector3(18.5, 43, 25.5);
const INITIAL_TARGET = new THREE.Vector3(0, 0.2, 1.5);

export function createArchitectureRuntime(
  container: HTMLDivElement,
  { graph, initialJourney, initialSelectedNodeId, autoRotate, onHover, onSelect }: ArchitectureRuntimeOptions
): ArchitectureRuntime {
  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true,
    powerPreference: 'high-performance',
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setClearColor(0x07100d, 0);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.08;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  container.appendChild(renderer.domElement);

  const scene = new THREE.Scene();
  scene.fog = new THREE.FogExp2(0x07100d, 0.011);
  const camera = new THREE.PerspectiveCamera(39, 1, 0.1, 180);
  camera.position.copy(INITIAL_CAMERA);
  camera.lookAt(INITIAL_TARGET);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.copy(INITIAL_TARGET);
  controls.enableDamping = true;
  controls.dampingFactor = 0.055;
  controls.rotateSpeed = 0.54;
  controls.zoomSpeed = 0.72;
  controls.panSpeed = 0.62;
  controls.minDistance = 8.5;
  controls.maxDistance = 72;
  controls.minPolarAngle = 0.18;
  controls.maxPolarAngle = Math.PI * 0.46;
  controls.screenSpacePanning = true;

  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  let autoRotateRequested = autoRotate && !reducedMotion;
  controls.autoRotate = autoRotateRequested;
  controls.autoRotateSpeed = 0.28;

  const environment = createArchitectureEnvironment(scene, graph.domains);
  const domains = new Map(graph.domains.map((domain) => [domain.id, domain]));
  const nodes = new Map(graph.nodes.map((node) => [node.id, node]));
  const journeys = new Map(graph.journeys.map((journey) => [journey.id, journey]));
  const carrierVisuals = ARCHITECTURE_CARRIERS.map((carrier) => {
    const visual = createArchitectureCarrierVisual(carrier, nodes);
    scene.add(visual.group);
    return visual;
  });
  const nodeVisuals = new Map(
    graph.nodes.map((node) => {
      const domain = domains.get(node.domain);
      if (!domain) throw new Error(`Architecture node ${node.id} references a missing domain`);
      const visual = createArchitectureNodeVisual(node, domain);
      scene.add(visual.group);
      return [node.id, visual] as const;
    })
  );
  const edgeVisuals = graph.edges.map((edge) => {
    const visual = createArchitectureEdgeVisual(edge, nodes, journeys);
    scene.add(visual.group);
    return visual;
  });
  const hostingVisuals = ARCHITECTURE_HOSTING_RELATIONSHIPS.map((relationship) => {
    const visual = createArchitectureHostingVisual(relationship, nodes);
    scene.add(visual.group);
    return visual;
  });

  let selectedNodeId = nodeVisuals.has(initialSelectedNodeId ?? '') ? initialSelectedNodeId : undefined;
  let hoveredNodeId: string | undefined;
  let activeJourney = initialJourney;
  let elapsed = 0;
  let flight: CameraFlight | undefined;
  let disposed = false;
  let animationFrame = 0;
  const timer = new THREE.Timer();
  timer.connect(document);

  const setSelectedNode = (nodeId?: string) => {
    selectedNodeId = nodeId && nodeVisuals.has(nodeId) ? nodeId : undefined;
    for (const visual of nodeVisuals.values()) visual.selected = visual.node.id === selectedNodeId;
    for (const visual of hostingVisuals) {
      visual.selected =
        selectedNodeId !== undefined &&
        (visual.relationship.hostNodeIds.includes(selectedNodeId) ||
          visual.relationship.guestNodeIds.includes(selectedNodeId));
    }
  };

  const setHoveredNode = (nodeId?: string) => {
    hoveredNodeId = nodeId && nodeVisuals.has(nodeId) ? nodeId : undefined;
    for (const visual of nodeVisuals.values()) visual.hovered = visual.node.id === hoveredNodeId;
  };

  const setSimulationFrame = (frame?: ArchitectureSimulationFrame) => {
    const nodeIds = new Set(frame?.nodeIds);
    const edgeIds = new Set(frame?.edgeIds);
    for (const visual of nodeVisuals.values()) {
      visual.simulationRunning = frame !== undefined;
      visual.spotlighted = nodeIds.has(visual.node.id);
    }
    for (const visual of edgeVisuals) {
      visual.simulationRunning = frame !== undefined;
      visual.spotlighted = edgeIds.has(visual.edge.id);
    }
    for (const visual of carrierVisuals) {
      visual.spotlighted = visual.carrier.memberNodeIds.some((nodeId) => nodeIds.has(nodeId));
    }
    for (const visual of hostingVisuals) {
      visual.spotlighted =
        frame !== undefined &&
        visual.relationship.hostNodeIds.some((nodeId) => nodeIds.has(nodeId)) &&
        visual.relationship.guestNodeIds.some((nodeId) => nodeIds.has(nodeId));
    }
  };

  const resetCamera = () => {
    flight = createCameraFlight(camera.position, controls.target, INITIAL_CAMERA, INITIAL_TARGET, elapsed);
  };

  const interaction = attachArchitectureInteraction({
    canvas: renderer.domElement,
    camera,
    targets: [...nodeVisuals.values()].flatMap((visual) => visual.hitTargets),
    onHover: (event) => {
      setHoveredNode(event?.nodeId);
      onHover(event);
    },
    onSelect: (nodeId) => {
      setSelectedNode(nodeId);
      onSelect(nodeId);
    },
    onReset: resetCamera,
  });

  const resize = () => {
    const width = Math.max(1, container.clientWidth);
    const height = Math.max(1, container.clientHeight);
    camera.aspect = width / height;
    camera.fov = camera.aspect < 0.82 ? 56 : camera.aspect < 1.2 ? 47 : 39;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height, false);
  };
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(container);
  resize();

  const animate = (timestamp?: number) => {
    if (disposed) return;
    animationFrame = window.requestAnimationFrame(animate);
    timer.update(timestamp);
    const delta = Math.min(timer.getDelta(), 0.05);
    elapsed += delta;
    updateCameraFlight(camera, controls, flight, elapsed);
    if (flight && elapsed >= flight.startedAt + flight.duration) flight = undefined;
    controls.autoRotate = autoRotateRequested && !flight;
    controls.update();
    for (const visual of nodeVisuals.values()) updateArchitectureNodeVisual(visual, elapsed);
    for (const visual of carrierVisuals) {
      updateArchitectureCarrierVisual(visual, elapsed, reducedMotion);
    }
    for (const visual of edgeVisuals) {
      updateArchitectureEdgeVisual(visual, elapsed, reducedMotion, selectedNodeId);
    }
    for (const visual of hostingVisuals) {
      updateArchitectureHostingVisual(visual, elapsed, reducedMotion);
    }
    updateArchitectureEnvironment(environment, elapsed, reducedMotion);
    renderer.render(scene, camera);
  };

  const setJourney = (journey: JourneyId) => {
    if (!journeys.has(journey)) return;
    activeJourney = journey;
    const activeNodes = nodeIdsForJourney(journey, graph.edges);
    for (const visual of nodeVisuals.values()) {
      visual.active = activeJourney === 'all' || activeNodes.has(visual.node.id);
    }
    for (const visual of edgeVisuals) {
      setArchitectureEdgeJourney(visual, activeJourney, journeys);
    }
  };

  setSelectedNode(selectedNodeId);
  setJourney(activeJourney);
  animate();

  return {
    focusNode: (nodeId) => {
      const node = nodes.get(nodeId);
      if (!node) return;
      setSelectedNode(nodeId);
      const target = new THREE.Vector3().fromArray(node.position);
      const cameraOffset = new THREE.Vector3(5.8, 9.2, 7.2);
      flight = createCameraFlight(
        camera.position,
        controls.target,
        target.clone().add(cameraOffset),
        target,
        elapsed
      );
    },
    resetCamera,
    setAutoRotate: (enabled) => {
      autoRotateRequested = enabled && !reducedMotion;
    },
    setJourney,
    setSelectedNode,
    setSimulationFrame,
    dispose: () => {
      disposed = true;
      window.cancelAnimationFrame(animationFrame);
      resizeObserver.disconnect();
      interaction.dispose();
      controls.dispose();
      timer.dispose();
      for (const visual of nodeVisuals.values()) {
        scene.remove(visual.group);
        disposeArchitectureNodeVisual(visual);
      }
      for (const visual of edgeVisuals) {
        scene.remove(visual.group);
        disposeArchitectureEdgeVisual(visual);
      }
      for (const visual of carrierVisuals) {
        scene.remove(visual.group);
        disposeArchitectureCarrierVisual(visual);
      }
      for (const visual of hostingVisuals) {
        scene.remove(visual.group);
        disposeArchitectureHostingVisual(visual);
      }
      disposeArchitectureEnvironment(scene, environment);
      renderer.dispose();
      renderer.domElement.remove();
    },
  };
}

function createCameraFlight(
  fromCamera: THREE.Vector3,
  fromTarget: THREE.Vector3,
  toCamera: THREE.Vector3,
  toTarget: THREE.Vector3,
  startedAt: number
): CameraFlight {
  return {
    startedAt,
    duration: 0.86,
    fromCamera: fromCamera.clone(),
    fromTarget: fromTarget.clone(),
    toCamera: toCamera.clone(),
    toTarget: toTarget.clone(),
  };
}

function updateCameraFlight(
  camera: THREE.PerspectiveCamera,
  controls: OrbitControls,
  flight: CameraFlight | undefined,
  elapsed: number
): void {
  if (!flight) return;
  const progress = THREE.MathUtils.clamp((elapsed - flight.startedAt) / flight.duration, 0, 1);
  const eased = 1 - (1 - progress) ** 3;
  camera.position.lerpVectors(flight.fromCamera, flight.toCamera, eased);
  controls.target.lerpVectors(flight.fromTarget, flight.toTarget, eased);
}
