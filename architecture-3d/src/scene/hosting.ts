import * as THREE from 'three';
import type { ArchitectureNode } from '../architecture';
import type { ArchitectureHostingRelationship } from '../topology';
import { createTextSprite, disposeTextSprite } from './text-sprite';

export interface ArchitectureHostingVisual {
  relationship: ArchitectureHostingRelationship;
  group: THREE.Group;
  paths: readonly THREE.Line<THREE.BufferGeometry, THREE.LineDashedMaterial>[];
  hitTargets: readonly THREE.Mesh<THREE.TubeGeometry, THREE.MeshBasicMaterial>[];
  joints: readonly THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>[];
  label: THREE.Sprite;
  hovered: boolean;
  spotlighted: boolean;
  selected: boolean;
}

export function createArchitectureHostingVisual(
  relationship: ArchitectureHostingRelationship,
  nodes: ReadonlyMap<string, ArchitectureNode>
): ArchitectureHostingVisual {
  const group = new THREE.Group();
  group.name = `hosting:${relationship.id}`;
  const lineMaterial = new THREE.LineDashedMaterial({
    color: relationship.color,
    dashSize: 0.32,
    gapSize: 0.19,
    transparent: true,
    opacity: 0.14,
    depthWrite: false,
  });
  const jointMaterial = new THREE.MeshBasicMaterial({
    color: relationship.color,
    transparent: true,
    opacity: 0.2,
    side: THREE.DoubleSide,
    depthWrite: false,
  });
  const paths: THREE.Line<THREE.BufferGeometry, THREE.LineDashedMaterial>[] = [];
  const hitTargets: THREE.Mesh<THREE.TubeGeometry, THREE.MeshBasicMaterial>[] = [];
  const joints: THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>[] = [];
  const midpoints: THREE.Vector3[] = [];

  for (const hostNodeId of relationship.hostNodeIds) {
    const host = nodes.get(hostNodeId);
    if (!host) continue;
    for (const guestNodeId of relationship.guestNodeIds) {
      const guest = nodes.get(guestNodeId);
      if (!guest || guest.id === host.id) continue;
      const start = new THREE.Vector3().fromArray(host.position);
      const end = new THREE.Vector3().fromArray(guest.position);
      const direction = end.clone().sub(start).normalize();
      start.addScaledVector(direction, 0.9);
      end.addScaledVector(direction, -0.9);
      start.y += 0.14;
      end.y += 0.14;
      const midpoint = start.clone().lerp(end, 0.5);
      midpoint.y += 0.42 + Math.min(start.distanceTo(end) * 0.025, 0.54);
      const curve = new THREE.QuadraticBezierCurve3(start, midpoint, end);
      const geometry = new THREE.BufferGeometry().setFromPoints(curve.getPoints(28));
      const path = new THREE.Line(geometry, lineMaterial);
      path.computeLineDistances();
      path.renderOrder = 2;
      group.add(path);
      paths.push(path);
      midpoints.push(midpoint);

      const hitTarget = new THREE.Mesh(
        new THREE.TubeGeometry(curve, 28, 0.22, 7, false),
        new THREE.MeshBasicMaterial({
          transparent: true,
          opacity: 0,
          depthWrite: false,
          colorWrite: false,
        })
      );
      hitTarget.name = `structural-edge-hit:${relationship.id}`;
      hitTarget.userData.architectureSelection = {
        kind: 'structural-edge',
        id: relationship.id,
      };
      group.add(hitTarget);
      hitTargets.push(hitTarget);

      const joint = new THREE.Mesh(new THREE.RingGeometry(0.34, 0.42, 28), jointMaterial);
      joint.position.copy(end);
      joint.position.y = 0.43;
      joint.rotation.x = -Math.PI / 2;
      joint.renderOrder = 3;
      group.add(joint);
      joints.push(joint);
    }
  }

  const label = createTextSprite(`${relationship.hostAction.toUpperCase()} · ${relationship.label}`, {
    color: relationship.color,
    fontSize: 25,
    fontWeight: 760,
    maxWidth: 760,
    scale: 0.00225,
    uppercase: true,
  });
  if (midpoints.length > 0) {
    label.position.copy(
      midpoints
        .reduce((sum, point) => sum.add(point), new THREE.Vector3())
        .multiplyScalar(1 / midpoints.length)
    );
  }
  label.position.y += 0.12;
  label.material.opacity = 0.34;
  group.add(label);

  return {
    relationship,
    group,
    paths,
    hitTargets,
    joints,
    label,
    hovered: false,
    spotlighted: false,
    selected: false,
  };
}

export function updateArchitectureHostingVisual(
  visual: ArchitectureHostingVisual,
  elapsed: number,
  reducedMotion: boolean
): void {
  const emphasized = visual.spotlighted || visual.selected || visual.hovered;
  const pulse = reducedMotion ? 0.5 : 0.5 + Math.sin(elapsed * 1.8 + visual.paths.length) * 0.5;
  const lineOpacity = visual.selected ? 0.78 + pulse * 0.18 : emphasized ? 0.68 + pulse * 0.18 : 0.11;
  for (const path of visual.paths) {
    path.material.opacity = lineOpacity;
  }
  for (const joint of visual.joints) {
    joint.material.opacity = visual.selected ? 0.76 + pulse * 0.18 : emphasized ? 0.62 + pulse * 0.2 : 0.14;
    joint.scale.setScalar(emphasized ? 1 + pulse * 0.1 : 1);
  }
  visual.label.material.opacity = emphasized ? 1 : 0.26;
}

export function disposeArchitectureHostingVisual(visual: ArchitectureHostingVisual): void {
  const geometries = new Set<THREE.BufferGeometry>();
  const materials = new Set<THREE.Material>();
  for (const path of visual.paths) {
    geometries.add(path.geometry);
    materials.add(path.material);
  }
  for (const hitTarget of visual.hitTargets) {
    geometries.add(hitTarget.geometry);
    materials.add(hitTarget.material);
  }
  for (const joint of visual.joints) {
    geometries.add(joint.geometry);
    materials.add(joint.material);
  }
  for (const geometry of geometries) geometry.dispose();
  for (const material of materials) material.dispose();
  disposeTextSprite(visual.label);
}
