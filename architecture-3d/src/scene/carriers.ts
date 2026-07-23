import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';
import type { ArchitectureNode } from '../architecture';
import type { ArchitectureCarrier } from '../topology';
import { createTextSprite, disposeTextSprite } from './text-sprite';

export interface ArchitectureCarrierVisual {
  carrier: ArchitectureCarrier;
  group: THREE.Group;
  pad: THREE.Mesh<RoundedBoxGeometry, THREE.MeshPhysicalMaterial>;
  border: THREE.LineSegments<THREE.EdgesGeometry, THREE.LineBasicMaterial>;
  sockets: readonly THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>[];
  label: THREE.Sprite;
  eyebrow: THREE.Sprite;
  spotlighted: boolean;
}

export function createArchitectureCarrierVisual(
  carrier: ArchitectureCarrier,
  nodes: ReadonlyMap<string, ArchitectureNode>
): ArchitectureCarrierVisual {
  const [width, depth] = carrier.size;
  const group = new THREE.Group();
  group.name = `carrier:${carrier.id}`;
  group.position.set(...carrier.position);

  const color = new THREE.Color(carrier.color);
  const pad = new THREE.Mesh(
    new RoundedBoxGeometry(width, 0.14, depth, 4, 0.22),
    new THREE.MeshPhysicalMaterial({
      color: color.clone().multiplyScalar(0.12),
      emissive: color,
      emissiveIntensity: 0.04,
      metalness: 0.7,
      roughness: 0.36,
      transparent: true,
      opacity: 0.46,
      clearcoat: 0.55,
      clearcoatRoughness: 0.42,
    })
  );
  pad.receiveShadow = true;
  group.add(pad);

  const border = new THREE.LineSegments(
    new THREE.EdgesGeometry(pad.geometry, 24),
    new THREE.LineBasicMaterial({
      color,
      transparent: true,
      opacity: 0.48,
      depthWrite: false,
    })
  );
  pad.add(border);

  const railMaterial = new THREE.MeshStandardMaterial({
    color: color.clone().multiplyScalar(0.3),
    emissive: color,
    emissiveIntensity: 0.12,
    metalness: 0.8,
    roughness: 0.3,
    transparent: true,
    opacity: 0.72,
  });
  for (const [railWidth, railDepth, x, z] of [
    [width - 0.28, 0.07, 0, depth / 2 - 0.12],
    [width - 0.28, 0.07, 0, -depth / 2 + 0.12],
    [0.07, depth - 0.28, width / 2 - 0.12, 0],
    [0.07, depth - 0.28, -width / 2 + 0.12, 0],
  ]) {
    const rail = new THREE.Mesh(new THREE.BoxGeometry(railWidth, 0.12, railDepth), railMaterial);
    rail.position.set(x, 0.1, z);
    rail.castShadow = true;
    group.add(rail);
  }

  const sockets: THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>[] = [];
  for (const nodeId of carrier.memberNodeIds) {
    const node = nodes.get(nodeId);
    if (!node) continue;
    const socket = new THREE.Mesh(
      new THREE.RingGeometry(1.13, 1.22, 40),
      new THREE.MeshBasicMaterial({
        color,
        transparent: true,
        opacity: 0.19,
        side: THREE.DoubleSide,
        depthWrite: false,
      })
    );
    socket.position.set(
      node.position[0] - carrier.position[0],
      0.105,
      node.position[2] - carrier.position[2]
    );
    socket.rotation.x = -Math.PI / 2;
    group.add(socket);
    sockets.push(socket);
  }

  const label = createTextSprite(carrier.label, {
    color: '#eaf5ec',
    fontSize: 34,
    fontWeight: 760,
    maxWidth: 760,
    scale: 0.00255,
    uppercase: true,
  });
  label.position.set(-width / 2 + 2.2, 0.3, depth / 2 - 0.24);
  group.add(label);

  const eyebrow = createTextSprite(carrier.eyebrow, {
    color,
    fontSize: 24,
    fontWeight: 720,
    maxWidth: 720,
    scale: 0.00215,
    uppercase: true,
  });
  eyebrow.position.set(width / 2 - 2.05, 0.29, depth / 2 - 0.24);
  group.add(eyebrow);

  return { carrier, group, pad, border, sockets, label, eyebrow, spotlighted: false };
}

export function updateArchitectureCarrierVisual(
  visual: ArchitectureCarrierVisual,
  elapsed: number,
  reducedMotion: boolean
): void {
  const pulse = reducedMotion ? 0.5 : 0.5 + Math.sin(elapsed * 1.1 + visual.carrier.position[0]) * 0.5;
  visual.pad.material.emissiveIntensity = visual.spotlighted ? 0.18 + pulse * 0.08 : 0.035;
  visual.border.material.opacity = visual.spotlighted ? 0.82 : 0.4;
  for (const socket of visual.sockets) {
    socket.material.opacity = visual.spotlighted ? 0.44 + pulse * 0.12 : 0.16;
  }
}

export function disposeArchitectureCarrierVisual(visual: ArchitectureCarrierVisual): void {
  const geometries = new Set<THREE.BufferGeometry>();
  const materials = new Set<THREE.Material>();
  visual.group.traverse((object) => {
    if (object instanceof THREE.Sprite) return;
    if (object instanceof THREE.Mesh || object instanceof THREE.LineSegments) {
      geometries.add(object.geometry);
      if (Array.isArray(object.material)) {
        for (const material of object.material) materials.add(material);
      } else {
        materials.add(object.material);
      }
    }
  });
  for (const geometry of geometries) geometry.dispose();
  for (const material of materials) material.dispose();
  disposeTextSprite(visual.label);
  disposeTextSprite(visual.eyebrow);
}
