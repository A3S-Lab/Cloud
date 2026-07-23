import * as THREE from 'three';
import type { ArchitectureEdge, ArchitectureNode, Journey, JourneyId } from '../architecture';

export interface ArchitectureEdgeVisual {
  edge: ArchitectureEdge;
  group: THREE.Group;
  curve: THREE.CubicBezierCurve3;
  tube: THREE.Mesh<THREE.TubeGeometry, THREE.MeshBasicMaterial>;
  hitTarget: THREE.Mesh<THREE.TubeGeometry, THREE.MeshBasicMaterial>;
  arrow: THREE.Mesh<THREE.ConeGeometry, THREE.MeshBasicMaterial>;
  particle: THREE.Mesh<THREE.SphereGeometry, THREE.MeshBasicMaterial>;
  active: boolean;
  hovered: boolean;
  selected: boolean;
  spotlighted: boolean;
  simulationRunning: boolean;
  phase: number;
}

export function createArchitectureEdgeVisual(
  edge: ArchitectureEdge,
  nodes: ReadonlyMap<string, ArchitectureNode>,
  journeys: ReadonlyMap<JourneyId, Journey>
): ArchitectureEdgeVisual {
  const from = nodes.get(edge.from);
  const to = nodes.get(edge.to);
  if (!from || !to) throw new Error(`Architecture edge ${edge.id} references a missing node`);

  const start = new THREE.Vector3().fromArray(from.position);
  const end = new THREE.Vector3().fromArray(to.position);
  const direction = end.clone().sub(start).normalize();
  start.addScaledVector(direction, 0.72);
  end.addScaledVector(direction, -0.72);
  const bend = signedHash(edge.id) * 1.15;
  const lift = Math.max(0.8, start.distanceTo(end) * 0.1);
  const controlA = start
    .clone()
    .lerp(end, 0.34)
    .add(new THREE.Vector3(0, lift, bend));
  const controlB = start
    .clone()
    .lerp(end, 0.68)
    .add(new THREE.Vector3(0, lift, bend));
  const curve = new THREE.CubicBezierCurve3(start, controlA, controlB, end);
  const color = journeys.get(edge.journeys[0])?.color ?? '#91a398';
  const group = new THREE.Group();
  group.name = `edge:${edge.id}`;

  const tube = new THREE.Mesh(
    new THREE.TubeGeometry(curve, 48, 0.018, 5, false),
    new THREE.MeshBasicMaterial({
      color,
      transparent: true,
      opacity: 0.24,
      depthWrite: false,
    })
  );
  tube.renderOrder = 1;
  group.add(tube);

  const hitTarget = new THREE.Mesh(
    new THREE.TubeGeometry(curve, 48, 0.2, 7, false),
    new THREE.MeshBasicMaterial({
      transparent: true,
      opacity: 0,
      depthWrite: false,
      colorWrite: false,
    })
  );
  hitTarget.name = `business-edge-hit:${edge.id}`;
  hitTarget.userData.architectureSelection = { kind: 'business-edge', id: edge.id };
  group.add(hitTarget);

  const arrow = new THREE.Mesh(
    new THREE.ConeGeometry(0.075, 0.24, 12),
    new THREE.MeshBasicMaterial({
      color,
      transparent: true,
      opacity: 0.62,
      depthWrite: false,
    })
  );
  const arrowPosition = curve.getPointAt(0.86);
  const arrowDirection = curve.getTangentAt(0.86).normalize();
  arrow.position.copy(arrowPosition);
  arrow.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), arrowDirection);
  arrow.renderOrder = 2;
  group.add(arrow);

  const particle = new THREE.Mesh(
    new THREE.SphereGeometry(0.06, 12, 12),
    new THREE.MeshBasicMaterial({
      color,
      transparent: true,
      opacity: 0.94,
      depthWrite: false,
    })
  );
  particle.renderOrder = 3;
  group.add(particle);

  return {
    edge,
    group,
    curve,
    tube,
    hitTarget,
    arrow,
    particle,
    active: true,
    hovered: false,
    selected: false,
    spotlighted: false,
    simulationRunning: false,
    phase: Math.abs(signedHash(edge.id)),
  };
}

export function setArchitectureEdgeJourney(
  visual: ArchitectureEdgeVisual,
  journey: JourneyId,
  journeys: ReadonlyMap<JourneyId, Journey>
): void {
  visual.active = journey === 'all' || visual.edge.journeys.includes(journey);
  const color =
    journey === 'all' ? journeys.get(visual.edge.journeys[0])?.color : journeys.get(journey)?.color;
  if (color) {
    visual.tube.material.color.set(color);
    visual.arrow.material.color.set(color);
    visual.particle.material.color.set(color);
  }
}

export function updateArchitectureEdgeVisual(
  visual: ArchitectureEdgeVisual,
  elapsed: number,
  reducedMotion: boolean,
  selectedNodeId?: string
): void {
  const touchesSelection = selectedNodeId === visual.edge.from || selectedNodeId === visual.edge.to;
  const spotlighted = visual.simulationRunning && visual.spotlighted;
  const backgroundSimulationEdge = visual.simulationRunning && !visual.spotlighted;
  const visible = visual.active || visual.selected;
  visual.tube.material.opacity = visible
    ? spotlighted
      ? 0.96
      : visual.selected
        ? 0.92
        : backgroundSimulationEdge
          ? 0.045
          : touchesSelection
            ? 0.72
            : visual.hovered
              ? 0.6
              : 0.28
    : 0.012;
  visual.arrow.material.opacity = visible
    ? spotlighted
      ? 1
      : visual.selected
        ? 1
        : backgroundSimulationEdge
          ? 0.08
          : touchesSelection
            ? 0.95
            : visual.hovered
              ? 0.88
              : 0.62
    : 0.018;
  visual.arrow.visible = visible && (!visual.simulationRunning || visual.spotlighted || visual.selected);
  visual.particle.visible = visible && (!visual.simulationRunning || visual.spotlighted || visual.selected);
  if (!visible) return;

  const speed = reducedMotion
    ? 0
    : spotlighted
      ? 0.22
      : visual.selected
        ? 0.15
        : touchesSelection || visual.hovered
          ? 0.115
          : 0.075;
  const progress = speed === 0 ? visual.phase : (elapsed * speed + visual.phase) % 1;
  visual.particle.position.copy(visual.curve.getPointAt(progress));
  visual.particle.scale.setScalar(
    spotlighted ? 1.7 : visual.selected ? 1.55 : touchesSelection || visual.hovered ? 1.34 : 1
  );
  visual.particle.material.opacity = reducedMotion ? 0.52 : 0.94;
}

export function disposeArchitectureEdgeVisual(visual: ArchitectureEdgeVisual): void {
  visual.tube.geometry.dispose();
  visual.tube.material.dispose();
  visual.hitTarget.geometry.dispose();
  visual.hitTarget.material.dispose();
  visual.arrow.geometry.dispose();
  visual.arrow.material.dispose();
  visual.particle.geometry.dispose();
  visual.particle.material.dispose();
}

function signedHash(value: string): number {
  let hash = 2166136261;
  for (const character of value) {
    hash ^= character.charCodeAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return ((hash >>> 0) % 2001) / 1000 - 1;
}
