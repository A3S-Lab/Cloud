import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { type ArchitectureGraph, type JourneyId, nodeIdsForJourney } from '../architecture';
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
  dispose: () => void;
}

interface CameraFlight {
  startedAt: number;
  duration: number;
  fromCamera: THREE.Vector3;
  fromTarget: THREE.Vector3;
  toCamera: THREE.Vector3;
  toTarget: THREE.Vector3;
}

const INITIAL_CAMERA = new THREE.Vector3(17.8, 8.8, 26);
const INITIAL_TARGET = new THREE.Vector3(0, -1.1, 0);

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
  scene.fog = new THREE.FogExp2(0x07100d, 0.017);
  const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 140);
  camera.position.copy(INITIAL_CAMERA);
  camera.lookAt(INITIAL_TARGET);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.copy(INITIAL_TARGET);
  controls.enableDamping = true;
  controls.dampingFactor = 0.055;
  controls.rotateSpeed = 0.54;
  controls.zoomSpeed = 0.72;
  controls.panSpeed = 0.62;
  controls.minDistance = 6.2;
  controls.maxDistance = 48;
  controls.minPolarAngle = 0.12;
  controls.maxPolarAngle = Math.PI * 0.92;

  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  let autoRotateRequested = autoRotate && !reducedMotion;
  controls.autoRotate = autoRotateRequested;
  controls.autoRotateSpeed = 0.28;

  const environment = createArchitectureEnvironment(scene, graph.layers);
  const layers = new Map(graph.layers.map((layer) => [layer.id, layer]));
  const nodes = new Map(graph.nodes.map((node) => [node.id, node]));
  const journeys = new Map(graph.journeys.map((journey) => [journey.id, journey]));
  const nodeVisuals = new Map(
    graph.nodes.map((node) => {
      const layer = layers.get(node.layer);
      if (!layer) throw new Error(`Architecture node ${node.id} references a missing layer`);
      const visual = createArchitectureNodeVisual(node, layer);
      scene.add(visual.group);
      return [node.id, visual] as const;
    })
  );
  const edgeVisuals = graph.edges.map((edge) => {
    const visual = createArchitectureEdgeVisual(edge, nodes, journeys);
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
  };

  const setHoveredNode = (nodeId?: string) => {
    hoveredNodeId = nodeId && nodeVisuals.has(nodeId) ? nodeId : undefined;
    for (const visual of nodeVisuals.values()) visual.hovered = visual.node.id === hoveredNodeId;
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
    camera.fov = camera.aspect < 0.82 ? 53 : camera.aspect < 1.2 ? 46 : 40;
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
    for (const visual of edgeVisuals) {
      updateArchitectureEdgeVisual(visual, elapsed, reducedMotion, selectedNodeId);
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
      const layerBias = node.layer === 'provider' ? 1.5 : node.layer === 'interfaces' ? -1 : 0;
      const cameraOffset = new THREE.Vector3(5.8, 3.5 + layerBias, 7.2);
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
