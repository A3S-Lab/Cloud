import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';
import type { ArchitectureLayer, ArchitectureNode } from '../architecture';
import { ARCHITECTURE_STATUS_META, type ArchitectureStatus } from '../architecture';
import { createTextSprite, disposeTextSprite } from './text-sprite';

export interface ArchitectureNodeVisual {
  node: ArchitectureNode;
  group: THREE.Group;
  body: THREE.Mesh<RoundedBoxGeometry, THREE.MeshPhysicalMaterial>;
  border: THREE.LineSegments<THREE.EdgesGeometry, THREE.LineBasicMaterial>;
  beacon: THREE.Mesh<THREE.SphereGeometry, THREE.MeshBasicMaterial>;
  halo: THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>;
  label: THREE.Sprite;
  eyebrow: THREE.Sprite;
  hitTargets: readonly THREE.Object3D[];
  active: boolean;
  hovered: boolean;
  selected: boolean;
}

export function createArchitectureNodeVisual(
  node: ArchitectureNode,
  layer: ArchitectureLayer
): ArchitectureNodeVisual {
  const group = new THREE.Group();
  group.name = `node:${node.id}`;
  group.position.fromArray(node.position);
  group.userData.nodeId = node.id;

  const layerColor = new THREE.Color(layer.color);
  const statusColor = new THREE.Color(ARCHITECTURE_STATUS_META[node.status].color);
  const bodyMaterial = new THREE.MeshPhysicalMaterial({
    color: layerColor.clone().multiplyScalar(0.34),
    emissive: layerColor,
    emissiveIntensity: 0.13,
    metalness: 0.34,
    roughness: 0.42,
    transparent: true,
    opacity: 0.93,
    clearcoat: 0.65,
    clearcoatRoughness: 0.36,
  });
  const body = new THREE.Mesh(new RoundedBoxGeometry(2.38, 0.78, 1.34, 5, 0.16), bodyMaterial);
  body.castShadow = true;
  body.receiveShadow = true;
  body.userData.nodeId = node.id;
  body.name = node.label;
  group.add(body);

  const border = new THREE.LineSegments(
    new THREE.EdgesGeometry(body.geometry, 28),
    new THREE.LineBasicMaterial({
      color: layerColor.clone().lerp(new THREE.Color('#ffffff'), 0.34),
      transparent: true,
      opacity: 0.46,
    })
  );
  border.userData.nodeId = node.id;
  body.add(border);

  const beacon = new THREE.Mesh(
    new THREE.SphereGeometry(0.075, 18, 18),
    new THREE.MeshBasicMaterial({ color: statusColor })
  );
  beacon.position.set(-0.94, 0.24, 0.69);
  beacon.userData.nodeId = node.id;
  group.add(beacon);

  const halo = new THREE.Mesh(
    new THREE.RingGeometry(0.92, 1.16, 56),
    new THREE.MeshBasicMaterial({
      color: statusColor,
      transparent: true,
      opacity: 0.12,
      side: THREE.DoubleSide,
      depthWrite: false,
    })
  );
  halo.rotation.x = -Math.PI / 2;
  halo.position.y = -0.48;
  halo.userData.nodeId = node.id;
  group.add(halo);

  const label = createTextSprite(node.label, {
    color: '#f3f8f4',
    fontSize: 42,
    fontWeight: 700,
    maxWidth: 720,
    scale: 0.0037,
  });
  label.position.set(0, 0.06, 0.72);
  label.userData.nodeId = node.id;
  group.add(label);

  const eyebrow = createTextSprite(node.gate, {
    color: statusColor,
    fontSize: 31,
    fontWeight: 760,
    maxWidth: 420,
    scale: 0.0027,
    uppercase: true,
  });
  eyebrow.position.set(0.72, -0.23, 0.72);
  eyebrow.userData.nodeId = node.id;
  group.add(eyebrow);

  return {
    node,
    group,
    body,
    border,
    beacon,
    halo,
    label,
    eyebrow,
    hitTargets: [body],
    active: true,
    hovered: false,
    selected: false,
  };
}

export function updateArchitectureNodeVisual(visual: ArchitectureNodeVisual, elapsed: number): void {
  const activeOpacity = visual.active ? 0.93 : 0.16;
  const emphasis = visual.selected ? 1 : visual.hovered ? 0.72 : 0;
  const pulse = 0.5 + Math.sin(elapsed * 2.2 + hashOffset(visual.node.id)) * 0.5;
  visual.body.material.opacity = activeOpacity;
  visual.body.material.emissiveIntensity = visual.active ? 0.13 + emphasis * 0.52 : 0.018;
  visual.border.material.opacity = visual.active ? 0.34 + emphasis * 0.58 : 0.06;
  visual.beacon.material.opacity = visual.active ? 1 : 0.18;
  visual.beacon.material.transparent = !visual.active;
  visual.halo.material.opacity = visual.active
    ? visual.selected
      ? 0.36 + pulse * 0.16
      : visual.hovered
        ? 0.28
        : 0.1
    : 0.025;
  visual.halo.scale.setScalar(visual.selected ? 1 + pulse * 0.08 : 1);
  visual.label.material.opacity = visual.active ? 1 : 0.18;
  visual.eyebrow.material.opacity = visual.active ? 0.94 : 0.16;
  visual.group.scale.lerp(
    new THREE.Vector3(
      visual.selected ? 1.1 : visual.hovered ? 1.045 : 1,
      visual.selected ? 1.1 : visual.hovered ? 1.045 : 1,
      visual.selected ? 1.1 : visual.hovered ? 1.045 : 1
    ),
    0.16
  );
}

export function disposeArchitectureNodeVisual(visual: ArchitectureNodeVisual): void {
  visual.body.geometry.dispose();
  visual.body.material.dispose();
  visual.border.geometry.dispose();
  visual.border.material.dispose();
  visual.beacon.geometry.dispose();
  visual.beacon.material.dispose();
  visual.halo.geometry.dispose();
  visual.halo.material.dispose();
  disposeTextSprite(visual.label);
  disposeTextSprite(visual.eyebrow);
}

export function statusColor(status: ArchitectureStatus): string {
  return ARCHITECTURE_STATUS_META[status].color;
}

function hashOffset(value: string): number {
  let hash = 0;
  for (const character of value) hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  return (hash % 628) / 100;
}
