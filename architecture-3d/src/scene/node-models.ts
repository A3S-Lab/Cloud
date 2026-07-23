import * as THREE from 'three';
import type { ArchitectureVisualKind } from '../architecture';

type FacilityMaterial = THREE.MeshStandardMaterial | THREE.MeshPhysicalMaterial;
type AnimationKind = 'bob' | 'pulse' | 'spin-x' | 'spin-y' | 'spin-z';

interface AnimatedPart {
  object: THREE.Object3D;
  kind: AnimationKind;
  speed: number;
  phase: number;
  originY: number;
}

export interface FacilityModel {
  group: THREE.Group;
  hitTargets: readonly THREE.Object3D[];
  materials: readonly FacilityMaterial[];
  animatedParts: readonly AnimatedPart[];
}

interface ModelKit {
  root: THREE.Group;
  primary: THREE.MeshStandardMaterial;
  accent: THREE.MeshStandardMaterial;
  dark: THREE.MeshStandardMaterial;
  glass: THREE.MeshPhysicalMaterial;
  animated: AnimatedPart[];
}

export function createFacilityModel(
  visualKind: ArchitectureVisualKind,
  color: THREE.ColorRepresentation,
  nodeId: string
): FacilityModel {
  const kit = createModelKit(color);

  switch (visualKind) {
    case 'client-terminal':
      addTerminal(kit, false);
      addBox(kit.root, [0.58, 0.32, 0.42], [-0.7, 0.3, 0.18], kit.dark);
      addBox(kit.root, [0.58, 0.32, 0.42], [0.7, 0.3, 0.18], kit.dark);
      break;
    case 'web-console':
      addBox(kit.root, [0.72, 1.65, 0.7], [0, 0.83, 0], kit.primary);
      addBox(kit.root, [0.08, 1.06, 0.84], [-0.52, 0.94, 0.06], kit.glass, [0, 0, -0.12]);
      addBox(kit.root, [0.08, 1.06, 0.84], [0.52, 0.94, 0.06], kit.glass, [0, 0, 0.12]);
      addBeacon(kit, [0, 1.78, 0]);
      break;
    case 'box-runtime':
      addBox(kit.root, [1.62, 1.24, 1.24], [0, 0.62, 0], kit.dark);
      addBox(kit.root, [1.3, 0.92, 0.08], [0, 0.64, 0.66], kit.glass);
      addBox(kit.root, [0.78, 0.62, 0.78], [0, 0.7, 0.02], kit.primary);
      for (const height of [0.44, 0.7, 0.96]) {
        addBox(kit.root, [0.64, 0.055, 0.66], [0, height, 0.04], kit.accent);
      }
      addBeacon(kit, [0.58, 1.42, 0.36]);
      break;
    case 'box-workload-host':
      addBox(kit.root, [2.18, 0.16, 1.5], [0, 0.08, 0], kit.dark);
      addBox(kit.root, [2.02, 1.3, 0.055], [0, 0.78, -0.68], kit.glass);
      for (const x of [-0.98, 0.98]) {
        for (const z of [-0.68, 0.68]) {
          addBox(kit.root, [0.11, 1.5, 0.11], [x, 0.78, z], kit.primary);
        }
      }
      for (const z of [-0.68, 0.68]) {
        addBox(kit.root, [2.08, 0.11, 0.11], [0, 1.5, z], kit.accent);
      }
      for (const [index, x] of [-0.62, 0, 0.62].entries()) {
        const unit = addCylinder(kit.root, [0.25, 0.25, 0.78, 18], [x, 0.62, 0.04], kit.primary);
        addSphere(kit.root, 0.25, [x, 1.01, 0.04], kit.accent);
        addSphere(kit.root, 0.25, [x, 0.23, 0.04], kit.dark);
        animate(kit, unit, 'pulse', 1.15, index * 0.75);
      }
      addBeacon(kit, [0.86, 1.66, 0.58]);
      break;
    case 'code-terminal':
      addTerminal(kit, true);
      addBox(kit.root, [1.25, 0.12, 0.68], [0, 0.2, 0.72], kit.dark, [-0.18, 0, 0]);
      for (const offset of [-0.42, -0.14, 0.14, 0.42]) {
        addBox(kit.root, [0.16, 0.04, 0.08], [offset, 0.31, 0.76], kit.accent);
      }
      break;
    case 'source-repository':
      addBox(kit.root, [1.45, 1.22, 1.08], [0, 0.61, 0], kit.dark);
      addBox(kit.root, [1.22, 0.16, 0.12], [0, 0.9, 0.59], kit.accent);
      addBranch(kit, 1.45);
      break;
    case 'inference-control':
      addCylinder(kit.root, [0.64, 0.76, 0.42, 24], [0, 0.21, 0], kit.dark);
      for (const [index, angle] of [0, (Math.PI * 2) / 3, (Math.PI * 4) / 3].entries()) {
        const x = Math.cos(angle) * 0.72;
        const z = Math.sin(angle) * 0.72;
        const model = addSphere(kit.root, 0.25, [x, 0.92, z], kit.primary);
        addCylinder(kit.root, [0.035, 0.035, 0.78, 8], [x / 2, 0.64, z / 2], kit.accent, [
          Math.PI / 2,
          -angle,
          0,
        ]);
        animate(kit, model, 'pulse', 1.1, index * 0.8);
      }
      addSphere(kit.root, 0.34, [0, 0.92, 0], kit.accent);
      break;
    case 'gpu-cluster':
      for (const [index, offset] of [-0.65, 0, 0.65].entries()) {
        const board = addBox(kit.root, [0.48, 1.28, 0.92], [offset, 0.68, 0], kit.primary);
        addCylinder(kit.root, [0.18, 0.18, 0.07, 24], [offset, 0.82, 0.5], kit.dark, [Math.PI / 2, 0, 0]);
        addCylinder(kit.root, [0.18, 0.18, 0.07, 24], [offset, 0.42, 0.5], kit.dark, [Math.PI / 2, 0, 0]);
        animate(kit, board, 'pulse', 1.2, index * 0.8);
      }
      break;
    case 'control-tower':
      addCylinder(kit.root, [0.7, 0.9, 1.5, 8], [0, 0.75, 0], kit.primary);
      addCylinder(kit.root, [0.98, 0.98, 0.22, 8], [0, 1.55, 0], kit.dark);
      addCone(kit.root, [0.72, 0.44, 8], [0, 1.88, 0], kit.accent);
      addAntenna(kit, 2.1);
      break;
    case 'identity-vault':
      addBox(kit.root, [1.52, 1.24, 1.16], [0, 0.62, 0], kit.dark);
      addCylinder(kit.root, [0.42, 0.42, 0.13, 24], [0, 0.67, 0.65], kit.accent, [Math.PI / 2, 0, 0]);
      addTorus(kit.root, [0.22, 0.07, 16], [0, 0.67, 0.74], kit.primary, [Math.PI / 2, 0, 0]);
      addBox(kit.root, [0.15, 0.35, 0.12], [0, 0.48, 0.75], kit.primary);
      break;
    case 'project-blocks':
      addBox(kit.root, [1.1, 0.42, 1.1], [-0.35, 0.21, 0.1], kit.dark);
      addBox(kit.root, [1, 0.62, 1], [0.34, 0.31, -0.2], kit.primary);
      addBox(kit.root, [0.76, 0.74, 0.76], [0, 0.99, 0.02], kit.accent);
      break;
    case 'source-branch':
      addBranch(kit, 0.25);
      addCylinder(kit.root, [0.58, 0.68, 0.28, 8], [0, 0.14, 0], kit.dark);
      break;
    case 'artifact-factory':
      addBox(kit.root, [1.3, 0.92, 1.12], [-0.2, 0.46, 0], kit.primary);
      addCylinder(kit.root, [0.18, 0.24, 0.82, 10], [0.38, 1.2, -0.2], kit.dark);
      addBox(kit.root, [0.92, 0.14, 0.48], [0.72, 0.2, 0.5], kit.dark);
      addBox(kit.root, [0.34, 0.34, 0.34], [0.72, 0.44, 0.5], kit.accent);
      break;
    case 'workload-cluster':
      for (const [index, offset] of [-0.68, 0, 0.68].entries()) {
        const pod = addCylinder(kit.root, [0.34, 0.34, 1.08, 16], [offset, 0.58, 0], kit.primary);
        addSphere(kit.root, 0.34, [offset, 1.11, 0], kit.accent);
        animate(kit, pod, 'pulse', 1.1, index * 0.75);
      }
      break;
    case 'fleet-radar':
      addCylinder(kit.root, [0.68, 0.82, 0.4, 24], [0, 0.2, 0], kit.dark);
      addCylinder(kit.root, [0.1, 0.14, 1.05, 12], [0, 0.88, 0], kit.primary);
      {
        const dish = addCylinder(kit.root, [0.68, 0.14, 0.18, 28], [0, 1.46, 0], kit.accent, [
          Math.PI / 2.8,
          0,
          0,
        ]);
        animate(kit, dish, 'spin-y', 0.65);
      }
      addTorus(kit.root, [0.9, 0.025, 48], [0, 0.1, 0], kit.accent, [Math.PI / 2, 0, 0]);
      break;
    case 'edge-router':
      addGateway(kit, 1.28);
      addSphere(kit.root, 0.12, [-0.64, 0.42, 0.52], kit.accent);
      addSphere(kit.root, 0.12, [0, 0.42, 0.52], kit.accent);
      addSphere(kit.root, 0.12, [0.64, 0.42, 0.52], kit.accent);
      break;
    case 'operations-timeline':
      addBox(kit.root, [1.8, 0.2, 0.68], [0, 0.1, 0], kit.dark);
      for (const [index, offset] of [-0.7, 0, 0.7].entries()) {
        addCylinder(kit.root, [0.08, 0.08, 0.55 + index * 0.24, 12], [offset, 0.38, 0], kit.primary);
        const marker = addSphere(kit.root, 0.17, [offset, 0.72 + index * 0.24, 0], kit.accent);
        animate(kit, marker, 'pulse', 1.4, index * 0.7);
      }
      break;
    case 'database':
      for (const [index, height] of [0.24, 0.67, 1.1].entries()) {
        const disk = addCylinder(
          kit.root,
          [0.82, 0.82, 0.36, 32],
          [0, height, 0],
          index % 2 ? kit.primary : kit.dark
        );
        animate(kit, disk, 'pulse', 0.7, index * 0.8);
      }
      break;
    case 'workflow-orchestrator':
      {
        const outer = addTorus(kit.root, [0.72, 0.11, 32], [0, 0.88, 0], kit.primary, [Math.PI / 2, 0, 0]);
        animate(kit, outer, 'spin-z', 0.42);
        const inner = addTorus(kit.root, [0.4, 0.08, 24], [0, 0.88, 0], kit.accent, [Math.PI / 2, 0, 0]);
        animate(kit, inner, 'spin-z', -0.7);
      }
      addCylinder(kit.root, [0.62, 0.82, 0.3, 12], [0, 0.15, 0], kit.dark);
      break;
    case 'event-bus':
      addBox(kit.root, [1.8, 0.18, 0.28], [0, 0.52, 0], kit.primary);
      for (const [index, offset] of [-0.72, -0.24, 0.24, 0.72].entries()) {
        const event = addSphere(kit.root, 0.16, [offset, 0.88, 0], kit.accent);
        addCylinder(kit.root, [0.035, 0.035, 0.36, 8], [offset, 0.7, 0], kit.dark);
        animate(kit, event, 'bob', 1.6, index * 0.62);
      }
      break;
    case 'object-storage':
      for (const [index, [offsetX, offsetZ]] of [
        [-0.52, 0.1],
        [0.52, 0.1],
        [0, -0.42],
      ].entries()) {
        const store = addCylinder(kit.root, [0.46, 0.46, 1, 24], [offsetX, 0.5, offsetZ], kit.primary);
        addTorus(kit.root, [0.37, 0.045, 24], [offsetX, 0.64, offsetZ], kit.accent, [Math.PI / 2, 0, 0]);
        animate(kit, store, 'pulse', 0.8, index * 0.9);
      }
      break;
    case 'node-antenna':
      addCylinder(kit.root, [0.58, 0.72, 0.34, 16], [0, 0.17, 0], kit.dark);
      addAntenna(kit, 0.45);
      {
        const signal = addTorus(kit.root, [0.72, 0.025, 40], [0, 1.28, 0], kit.accent, [Math.PI / 2, 0, 0]);
        animate(kit, signal, 'pulse', 1.45);
      }
      break;
    case 'runtime-engine':
      addCylinder(kit.root, [0.58, 0.58, 1.25, 24], [0, 0.72, 0], kit.primary, [0, 0, Math.PI / 2]);
      {
        const gear = addTorus(kit.root, [0.68, 0.13, 24], [0, 0.72, 0], kit.accent, [0, Math.PI / 2, 0]);
        animate(kit, gear, 'spin-x', 0.9);
      }
      addCylinder(kit.root, [0.24, 0.24, 1.72, 18], [0, 0.72, 0], kit.dark, [0, 0, Math.PI / 2]);
      break;
    case 'traffic-gateway':
      addGateway(kit, 1.65);
      {
        const route = addTorus(kit.root, [0.48, 0.045, 30], [0, 0.82, 0.2], kit.accent, [Math.PI / 2, 0, 0]);
        animate(kit, route, 'pulse', 1.25);
      }
      break;
    case 'buildkit-yard':
      addBox(kit.root, [0.82, 0.58, 0.74], [-0.48, 0.29, 0.1], kit.primary);
      addBox(kit.root, [0.82, 0.58, 0.74], [0.48, 0.29, -0.12], kit.dark);
      addCylinder(kit.root, [0.07, 0.07, 1.48, 8], [-0.86, 0.74, -0.42], kit.accent);
      addBox(kit.root, [1.5, 0.08, 0.08], [-0.16, 1.46, -0.42], kit.accent);
      addCylinder(kit.root, [0.025, 0.025, 0.65, 8], [0.52, 1.14, -0.42], kit.accent);
      break;
    case 'healthy-runtime':
      addCylinder(kit.root, [0.58, 0.58, 1.12, 24], [0, 0.76, 0], kit.primary);
      addSphere(kit.root, 0.58, [0, 1.3, 0], kit.accent);
      addSphere(kit.root, 0.58, [0, 0.22, 0], kit.dark);
      addTorus(kit.root, [0.78, 0.045, 40], [0, 0.18, 0], kit.accent, [Math.PI / 2, 0, 0]);
      break;
    case 'registry-rack':
      addBox(kit.root, [1.48, 1.5, 0.94], [0, 0.75, 0], kit.dark);
      for (const [index, height] of [0.34, 0.72, 1.1].entries()) {
        addBox(kit.root, [1.2, 0.24, 0.12], [0, height, 0.54], index === 1 ? kit.accent : kit.primary);
      }
      break;
    case 'cpu-array':
      kit.accent.color.set('#8bc9ff');
      kit.accent.emissive.set('#4da7ff');
      addComputeRackCluster(kit, false);
      break;
    case 'gpu-array':
      kit.accent.color.set('#b69cff');
      kit.accent.emissive.set('#8e72ff');
      addComputeRackCluster(kit, true);
      break;
  }

  const hitTargets: THREE.Object3D[] = [];
  kit.root.traverse((object) => {
    object.userData.nodeId = nodeId;
    if (object instanceof THREE.Mesh) {
      object.castShadow = true;
      object.receiveShadow = true;
      hitTargets.push(object);
    }
  });

  return {
    group: kit.root,
    hitTargets,
    materials: [kit.primary, kit.accent, kit.dark, kit.glass],
    animatedParts: kit.animated,
  };
}

export function updateFacilityModel(model: FacilityModel, elapsed: number): void {
  for (const part of model.animatedParts) {
    const angle = elapsed * part.speed + part.phase;
    if (part.kind === 'spin-x') part.object.rotation.x = angle;
    if (part.kind === 'spin-y') part.object.rotation.y = angle;
    if (part.kind === 'spin-z') part.object.rotation.z = angle;
    if (part.kind === 'bob') part.object.position.y = part.originY + Math.sin(angle) * 0.09;
    if (part.kind === 'pulse') {
      const scale = 1 + Math.sin(angle) * 0.045;
      part.object.scale.setScalar(scale);
    }
  }
}

export function disposeFacilityModel(model: FacilityModel): void {
  const geometries = new Set<THREE.BufferGeometry>();
  model.group.traverse((object) => {
    if (object instanceof THREE.Mesh) geometries.add(object.geometry);
  });
  for (const geometry of geometries) geometry.dispose();
  for (const material of model.materials) material.dispose();
}

function createModelKit(color: THREE.ColorRepresentation): ModelKit {
  const baseColor = new THREE.Color(color);
  return {
    root: new THREE.Group(),
    primary: new THREE.MeshStandardMaterial({
      color: baseColor.clone().multiplyScalar(0.5),
      emissive: baseColor,
      emissiveIntensity: 0.12,
      metalness: 0.54,
      roughness: 0.36,
      transparent: true,
    }),
    accent: new THREE.MeshStandardMaterial({
      color: baseColor.clone().lerp(new THREE.Color('#ffffff'), 0.45),
      emissive: baseColor,
      emissiveIntensity: 0.36,
      metalness: 0.42,
      roughness: 0.25,
      transparent: true,
    }),
    dark: new THREE.MeshStandardMaterial({
      color: 0x101b15,
      emissive: baseColor,
      emissiveIntensity: 0.045,
      metalness: 0.68,
      roughness: 0.42,
      transparent: true,
    }),
    glass: new THREE.MeshPhysicalMaterial({
      color: baseColor.clone().lerp(new THREE.Color('#ffffff'), 0.3),
      emissive: baseColor,
      emissiveIntensity: 0.16,
      metalness: 0.05,
      roughness: 0.12,
      transmission: 0.14,
      transparent: true,
      opacity: 0.72,
      clearcoat: 0.8,
    }),
    animated: [],
  };
}

function addTerminal(kit: ModelKit, tall: boolean): void {
  const height = tall ? 1.32 : 1.08;
  addBox(kit.root, [1.5, height, 0.18], [0, 0.68, 0], kit.dark, [-0.04, 0, 0]);
  addBox(kit.root, [1.26, height - 0.25, 0.06], [0, 0.7, 0.13], kit.glass, [-0.04, 0, 0]);
  addCylinder(kit.root, [0.09, 0.09, 0.5, 12], [0, 0.19, -0.1], kit.primary);
  addBox(kit.root, [0.78, 0.12, 0.54], [0, 0.08, -0.04], kit.primary);
}

function addBranch(kit: ModelKit, baseHeight: number): void {
  addCylinder(kit.root, [0.07, 0.07, 1.3, 10], [0, baseHeight + 0.65, 0], kit.primary);
  addCylinder(kit.root, [0.055, 0.055, 0.86, 10], [0.36, baseHeight + 0.92, 0], kit.primary, [
    0,
    0,
    Math.PI / 2,
  ]);
  addSphere(kit.root, 0.16, [0, baseHeight + 1.32, 0], kit.accent);
  addSphere(kit.root, 0.16, [0.78, baseHeight + 0.92, 0], kit.accent);
  addSphere(kit.root, 0.16, [0, baseHeight + 0.18, 0], kit.accent);
}

function addAntenna(kit: ModelKit, baseHeight: number): void {
  addCylinder(kit.root, [0.055, 0.075, 1.25, 8], [0, baseHeight + 0.62, 0], kit.primary);
  addSphere(kit.root, 0.11, [0, baseHeight + 1.28, 0], kit.accent);
  addCylinder(kit.root, [0.035, 0.035, 0.9, 8], [0.22, baseHeight + 0.72, 0], kit.accent, [0, 0, -0.45]);
  addCylinder(kit.root, [0.035, 0.035, 0.9, 8], [-0.22, baseHeight + 0.72, 0], kit.accent, [0, 0, 0.45]);
}

function addGateway(kit: ModelKit, height: number): void {
  addBox(kit.root, [0.34, height, 0.7], [-0.72, height / 2, 0], kit.primary);
  addBox(kit.root, [0.34, height, 0.7], [0.72, height / 2, 0], kit.primary);
  addBox(kit.root, [1.78, 0.32, 0.7], [0, height - 0.16, 0], kit.dark);
}

function addComputeRackCluster(kit: ModelKit, gpu: boolean): void {
  addBox(kit.root, [2.18, 0.16, 1.22], [0, 0.08, 0], kit.dark);
  for (const [rackIndex, offset] of [-0.72, 0, 0.72].entries()) {
    addBox(kit.root, [0.58, 1.56, 0.78], [offset, 0.86, 0], kit.dark);
    addBox(kit.root, [0.48, 1.38, 0.055], [offset, 0.86, 0.42], kit.primary);
    for (const [shelfIndex, height] of [0.34, 0.67, 1, 1.33].entries()) {
      addBox(kit.root, [0.4, 0.08, 0.09], [offset, height, 0.47], kit.accent);
      if (!gpu) {
        addSphere(kit.root, 0.026, [offset + 0.14, height, 0.53], kit.accent);
      } else if (shelfIndex % 2 === 0) {
        const fan = addTorus(kit.root, [0.115, 0.028, 18], [offset, height + 0.11, 0.51], kit.accent);
        animate(kit, fan, 'spin-z', 2.1 + rackIndex * 0.2, shelfIndex * 0.6);
      }
    }
    addBeacon(kit, [offset + 0.18, 1.55, 0.45]);
  }
}

function addBeacon(kit: ModelKit, position: readonly [number, number, number]): void {
  const beacon = addSphere(kit.root, 0.13, position, kit.accent);
  animate(kit, beacon, 'pulse', 1.8);
}

function animate(kit: ModelKit, object: THREE.Object3D, kind: AnimationKind, speed: number, phase = 0): void {
  kit.animated.push({ object, kind, speed, phase, originY: object.position.y });
}

function addBox(
  root: THREE.Group,
  size: readonly [number, number, number],
  position: readonly [number, number, number],
  material: FacilityMaterial,
  rotation: readonly [number, number, number] = [0, 0, 0]
): THREE.Mesh {
  return addMesh(root, new THREE.BoxGeometry(...size), position, material, rotation);
}

function addCylinder(
  root: THREE.Group,
  geometry: readonly [number, number, number, number],
  position: readonly [number, number, number],
  material: FacilityMaterial,
  rotation: readonly [number, number, number] = [0, 0, 0]
): THREE.Mesh {
  return addMesh(root, new THREE.CylinderGeometry(...geometry), position, material, rotation);
}

function addCone(
  root: THREE.Group,
  geometry: readonly [number, number, number],
  position: readonly [number, number, number],
  material: FacilityMaterial
): THREE.Mesh {
  return addMesh(root, new THREE.ConeGeometry(...geometry), position, material);
}

function addSphere(
  root: THREE.Group,
  radius: number,
  position: readonly [number, number, number],
  material: FacilityMaterial
): THREE.Mesh {
  return addMesh(root, new THREE.SphereGeometry(radius, 18, 14), position, material);
}

function addTorus(
  root: THREE.Group,
  geometry: readonly [number, number, number],
  position: readonly [number, number, number],
  material: FacilityMaterial,
  rotation: readonly [number, number, number] = [0, 0, 0]
): THREE.Mesh {
  return addMesh(root, new THREE.TorusGeometry(...geometry), position, material, rotation);
}

function addMesh(
  root: THREE.Group,
  geometry: THREE.BufferGeometry,
  position: readonly [number, number, number],
  material: FacilityMaterial,
  rotation: readonly [number, number, number] = [0, 0, 0]
): THREE.Mesh {
  const mesh = new THREE.Mesh(geometry, material);
  mesh.position.set(...position);
  mesh.rotation.set(...rotation);
  root.add(mesh);
  return mesh;
}
