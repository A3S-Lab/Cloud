import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';
import {
  ARCHITECTURE_STATUS_META,
  type ArchitectureDomain,
  type ArchitectureNode,
  type ArchitectureStatus,
} from '../architecture';
import { createBrandSprite, disposeBrandSprite } from './brand-textures';
import {
  type FacilityModel,
  createFacilityModel,
  disposeFacilityModel,
  updateFacilityModel,
} from './node-models';
import { createTextSprite, disposeTextSprite } from './text-sprite';

export interface ArchitectureNodeVisual {
  node: ArchitectureNode;
  group: THREE.Group;
  body: THREE.Mesh<RoundedBoxGeometry, THREE.MeshPhysicalMaterial>;
  border: THREE.LineSegments<THREE.EdgesGeometry, THREE.LineBasicMaterial>;
  beacon: THREE.Mesh<THREE.SphereGeometry, THREE.MeshBasicMaterial>;
  halo: THREE.Mesh<THREE.RingGeometry, THREE.MeshBasicMaterial>;
  logo: THREE.Sprite;
  label: THREE.Sprite;
  gate: THREE.Sprite;
  facility: FacilityModel;
  hitTargets: readonly THREE.Object3D[];
  active: boolean;
  hovered: boolean;
  selected: boolean;
  spotlighted: boolean;
  simulationRunning: boolean;
}

export function createArchitectureNodeVisual(
  node: ArchitectureNode,
  domain: ArchitectureDomain
): ArchitectureNodeVisual {
  const group = new THREE.Group();
  group.name = `node:${node.id}`;
  group.position.fromArray(node.position);
  group.userData.nodeId = node.id;

  const domainColor = new THREE.Color(domain.color);
  const statusColor = new THREE.Color(ARCHITECTURE_STATUS_META[node.status].color);
  const bodyMaterial = new THREE.MeshPhysicalMaterial({
    color: domainColor.clone().multiplyScalar(0.22),
    emissive: domainColor,
    emissiveIntensity: 0.08,
    metalness: 0.52,
    roughness: 0.42,
    transparent: true,
    opacity: 0.96,
    clearcoat: 0.7,
    clearcoatRoughness: 0.32,
  });
  const body = new THREE.Mesh(new RoundedBoxGeometry(2.72, 0.24, 2.04, 4, 0.12), bodyMaterial);
  body.castShadow = true;
  body.receiveShadow = true;
  body.userData.nodeId = node.id;
  body.name = node.label;
  group.add(body);

  const border = new THREE.LineSegments(
    new THREE.EdgesGeometry(body.geometry, 28),
    new THREE.LineBasicMaterial({
      color: domainColor.clone().lerp(new THREE.Color('#ffffff'), 0.38),
      transparent: true,
      opacity: 0.48,
    })
  );
  border.userData.nodeId = node.id;
  body.add(border);

  const facility = createFacilityModel(node.visualKind, domain.color, node.id);
  facility.group.name = `facility:${node.visualKind}`;
  facility.group.position.y = 0.14;
  group.add(facility.group);
  for (const material of facility.materials) {
    material.userData.baseOpacity = material.opacity;
    material.userData.baseEmissiveIntensity = material.emissiveIntensity;
  }

  const beacon = new THREE.Mesh(
    new THREE.SphereGeometry(0.085, 18, 18),
    new THREE.MeshBasicMaterial({ color: statusColor })
  );
  beacon.position.set(-1.12, 0.24, 0.82);
  beacon.userData.nodeId = node.id;
  group.add(beacon);

  const halo = new THREE.Mesh(
    new THREE.RingGeometry(1.3, 1.48, 64),
    new THREE.MeshBasicMaterial({
      color: statusColor,
      transparent: true,
      opacity: 0.1,
      side: THREE.DoubleSide,
      depthWrite: false,
    })
  );
  halo.rotation.x = -Math.PI / 2;
  halo.position.y = -0.1;
  halo.userData.nodeId = node.id;
  group.add(halo);

  const modelBounds = new THREE.Box3().setFromObject(facility.group);
  const logo = createBrandSprite(node.logoId);
  logo.position.set(0, Math.max(1.75, modelBounds.max.y + 0.43), -0.05);
  logo.userData.nodeId = node.id;
  group.add(logo);

  const label = createTextSprite(node.label, {
    color: '#f3f8f4',
    fontSize: 38,
    fontWeight: 720,
    maxWidth: 700,
    scale: 0.0028,
  });
  label.position.set(0, 0.2, 1.24);
  label.userData.nodeId = node.id;
  group.add(label);

  const gate = createTextSprite(node.gate, {
    color: statusColor,
    fontSize: 27,
    fontWeight: 780,
    maxWidth: 420,
    scale: 0.00235,
    uppercase: true,
  });
  gate.position.set(0.74, 0.03, 1.24);
  gate.userData.nodeId = node.id;
  group.add(gate);

  return {
    node,
    group,
    body,
    border,
    beacon,
    halo,
    logo,
    label,
    gate,
    facility,
    hitTargets: [body, ...facility.hitTargets],
    active: true,
    hovered: false,
    selected: false,
    spotlighted: false,
    simulationRunning: false,
  };
}

export function updateArchitectureNodeVisual(visual: ArchitectureNodeVisual, elapsed: number): void {
  updateFacilityModel(visual.facility, elapsed);
  const visible = visual.active || (visual.simulationRunning && visual.spotlighted);
  const activeOpacity = visible
    ? visual.simulationRunning && !visual.spotlighted && !visual.selected
      ? 0.3
      : 1
    : 0.12;
  const emphasis = visual.selected ? 1 : visual.spotlighted ? 0.88 : visual.hovered ? 0.66 : 0;
  const pulse = 0.5 + Math.sin(elapsed * 2.2 + hashOffset(visual.node.id)) * 0.5;
  visual.body.material.opacity = 0.96 * activeOpacity;
  visual.body.material.emissiveIntensity = visible ? 0.08 + emphasis * 0.5 : 0.012;
  visual.border.material.opacity = visible ? 0.32 + emphasis * 0.62 : 0.04;
  visual.beacon.material.opacity = visible ? 1 : 0.15;
  visual.beacon.material.transparent = !visible;
  visual.halo.material.opacity = visible
    ? visual.selected
      ? 0.38 + pulse * 0.18
      : visual.spotlighted
        ? 0.3 + pulse * 0.12
        : visual.hovered
          ? 0.26
          : 0.075
    : 0.018;
  visual.halo.scale.setScalar(visual.selected || visual.spotlighted ? 1 + pulse * 0.08 : 1);
  visual.logo.material.opacity = visible ? (emphasis > 0 ? 1 : 0.9) : 0.1;
  visual.label.material.opacity = visible ? 0.94 : 0.1;
  visual.gate.material.opacity = visible ? 0.9 : 0.1;

  for (const material of visual.facility.materials) {
    const baseOpacity = Number(material.userData.baseOpacity ?? 1);
    const baseEmissive = Number(material.userData.baseEmissiveIntensity ?? 0);
    material.opacity = baseOpacity * activeOpacity;
    material.emissiveIntensity = visible ? baseEmissive + emphasis * 0.34 : baseEmissive * 0.08;
  }

  const targetScale = visual.selected ? 1.08 : visual.spotlighted ? 1.055 : visual.hovered ? 1.035 : 1;
  visual.group.scale.lerp(new THREE.Vector3(targetScale, targetScale, targetScale), 0.15);
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
  disposeFacilityModel(visual.facility);
  disposeBrandSprite(visual.logo);
  disposeTextSprite(visual.label);
  disposeTextSprite(visual.gate);
}

export function statusColor(status: ArchitectureStatus): string {
  return ARCHITECTURE_STATUS_META[status].color;
}

function hashOffset(value: string): number {
  let hash = 0;
  for (const character of value) hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
  return (hash % 628) / 100;
}
