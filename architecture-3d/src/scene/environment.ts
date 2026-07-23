import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';
import type { ArchitectureDomain } from '../architecture';
import { createTextSprite, disposeTextSprite } from './text-sprite';

interface DomainDistrict {
  group: THREE.Group;
  pad: THREE.Mesh<RoundedBoxGeometry, THREE.MeshPhysicalMaterial>;
  border: THREE.LineSegments<THREE.EdgesGeometry, THREE.LineBasicMaterial>;
  grid: THREE.GridHelper;
  label: THREE.Sprite;
  shortLabel: THREE.Sprite;
  description: THREE.Sprite;
}

export interface ArchitectureEnvironment {
  group: THREE.Group;
  districts: readonly DomainDistrict[];
  stars: THREE.Points<THREE.BufferGeometry, THREE.PointsMaterial>;
  lights: readonly THREE.Light[];
}

export function createArchitectureEnvironment(
  scene: THREE.Scene,
  domains: readonly ArchitectureDomain[]
): ArchitectureEnvironment {
  const group = new THREE.Group();
  group.name = 'architecture-sandbox';
  group.add(createFoundation());
  group.add(createRoadNetwork());
  group.add(createMapCompass());

  const districts = domains.map(createDomainDistrict);
  for (const district of districts) group.add(district.group);

  const stars = createStars();
  group.add(stars);

  const hemisphere = new THREE.HemisphereLight(0xc4f58b, 0x06100a, 1.55);
  const key = new THREE.DirectionalLight(0xe7ffd0, 3.2);
  key.position.set(-14, 27, 18);
  key.castShadow = true;
  key.shadow.mapSize.set(2048, 2048);
  key.shadow.camera.left = -22;
  key.shadow.camera.right = 22;
  key.shadow.camera.top = 20;
  key.shadow.camera.bottom = -20;
  key.shadow.camera.near = 2;
  key.shadow.camera.far = 58;
  const blueRim = new THREE.PointLight(0x72b7ff, 32, 48, 2);
  blueRim.position.set(16, 8, 13);
  const violetFill = new THREE.PointLight(0xd7b6ff, 24, 44, 2);
  violetFill.position.set(-15, 7, -12);
  const amberFill = new THREE.PointLight(0xf3c86b, 20, 36, 2);
  amberFill.position.set(14, 5, -10);
  const lights: readonly THREE.Light[] = [hemisphere, key, blueRim, violetFill, amberFill];
  scene.add(group, ...lights);
  return { group, districts, stars, lights };
}

export function updateArchitectureEnvironment(
  environment: ArchitectureEnvironment,
  elapsed: number,
  reducedMotion: boolean
): void {
  if (reducedMotion) return;
  environment.stars.rotation.y = elapsed * 0.0025;
  for (const [index, district] of environment.districts.entries()) {
    district.pad.material.emissiveIntensity = 0.045 + Math.sin(elapsed * 0.33 + index) * 0.012;
    district.border.material.opacity = 0.38 + Math.sin(elapsed * 0.4 + index * 0.7) * 0.06;
  }
}

export function disposeArchitectureEnvironment(
  scene: THREE.Scene,
  environment: ArchitectureEnvironment
): void {
  scene.remove(environment.group, ...environment.lights);
  const geometries = new Set<THREE.BufferGeometry>();
  const materials = new Set<THREE.Material>();
  environment.group.traverse((object) => {
    if (object instanceof THREE.Sprite) return;
    if (
      object instanceof THREE.Mesh ||
      object instanceof THREE.Line ||
      object instanceof THREE.LineSegments ||
      object instanceof THREE.Points
    ) {
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
  for (const district of environment.districts) {
    disposeTextSprite(district.label);
    disposeTextSprite(district.shortLabel);
    disposeTextSprite(district.description);
  }
}

function createFoundation(): THREE.Group {
  const group = new THREE.Group();
  group.name = 'sandbox-foundation';
  const foundation = new THREE.Mesh(
    new RoundedBoxGeometry(35.2, 0.52, 28.4, 5, 0.34),
    new THREE.MeshPhysicalMaterial({
      color: 0x0b1510,
      emissive: 0x152a1d,
      emissiveIntensity: 0.05,
      metalness: 0.66,
      roughness: 0.52,
      clearcoat: 0.46,
      clearcoatRoughness: 0.44,
    })
  );
  foundation.position.y = -0.42;
  foundation.receiveShadow = true;
  group.add(foundation);

  const outline = new THREE.LineSegments(
    new THREE.EdgesGeometry(foundation.geometry, 24),
    new THREE.LineBasicMaterial({
      color: 0x7ba98a,
      transparent: true,
      opacity: 0.28,
    })
  );
  foundation.add(outline);

  for (const z of [-13.55, 13.55]) {
    for (let x = -16.2; x <= 16.2; x += 1.2) {
      group.add(createPerimeterLight(x, -0.08, z));
    }
  }
  for (const x of [-16.9, 16.9]) {
    for (let z = -12.2; z <= 12.2; z += 1.2) {
      group.add(createPerimeterLight(x, -0.08, z));
    }
  }
  return group;
}

function createPerimeterLight(x: number, y: number, z: number): THREE.Mesh {
  const light = new THREE.Mesh(
    new THREE.SphereGeometry(0.035, 8, 8),
    new THREE.MeshBasicMaterial({
      color: 0xb8f36b,
      transparent: true,
      opacity: 0.56,
    })
  );
  light.position.set(x, y, z);
  return light;
}

function createRoadNetwork(): THREE.Group {
  const group = new THREE.Group();
  group.name = 'sandbox-roads';
  const roadMaterial = new THREE.MeshStandardMaterial({
    color: 0x08100c,
    emissive: 0x18271d,
    emissiveIntensity: 0.04,
    metalness: 0.34,
    roughness: 0.78,
  });
  const laneMaterial = new THREE.MeshBasicMaterial({
    color: 0x557563,
    transparent: true,
    opacity: 0.32,
  });

  addRoad(group, [32, 0.08, 0.72], [0, -0.06, 6.45], roadMaterial);
  addRoad(group, [32, 0.08, 0.72], [0, -0.06, -3.25], roadMaterial);
  addRoad(group, [0.72, 0.08, 8.2], [-5.35, -0.06, -8.1], roadMaterial);
  addRoad(group, [0.72, 0.08, 8.2], [5.35, -0.06, -8.1], roadMaterial);

  for (let x = -15.5; x <= 15.5; x += 1.15) {
    addRoad(group, [0.52, 0.015, 0.045], [x, -0.005, 6.45], laneMaterial);
    addRoad(group, [0.52, 0.015, 0.045], [x, -0.005, -3.25], laneMaterial);
  }
  for (let z = -11.6; z <= -4.6; z += 1.1) {
    addRoad(group, [0.045, 0.015, 0.48], [-5.35, -0.005, z], laneMaterial);
    addRoad(group, [0.045, 0.015, 0.48], [5.35, -0.005, z], laneMaterial);
  }
  return group;
}

function addRoad(
  group: THREE.Group,
  size: readonly [number, number, number],
  position: readonly [number, number, number],
  material: THREE.Material
): void {
  const road = new THREE.Mesh(new THREE.BoxGeometry(...size), material);
  road.position.set(...position);
  road.receiveShadow = true;
  group.add(road);
}

function createDomainDistrict(domain: ArchitectureDomain): DomainDistrict {
  const group = new THREE.Group();
  group.name = `domain:${domain.id}`;
  group.position.set(domain.center[0], 0, domain.center[1]);
  const [width, depth] = domain.size;

  const pad = new THREE.Mesh(
    new RoundedBoxGeometry(width, 0.24, depth, 4, 0.28),
    new THREE.MeshPhysicalMaterial({
      color: new THREE.Color(domain.color).multiplyScalar(0.13),
      emissive: domain.color,
      emissiveIntensity: 0.045,
      metalness: 0.42,
      roughness: 0.64,
      transparent: true,
      opacity: 0.86,
      clearcoat: 0.4,
      clearcoatRoughness: 0.5,
    })
  );
  pad.position.y = -0.06;
  pad.receiveShadow = true;
  group.add(pad);

  const border = new THREE.LineSegments(
    new THREE.EdgesGeometry(pad.geometry, 28),
    new THREE.LineBasicMaterial({
      color: domain.color,
      transparent: true,
      opacity: 0.4,
      depthWrite: false,
    })
  );
  pad.add(border);

  const grid = new THREE.GridHelper(width, Math.max(8, Math.round(width)), domain.color, 0x24372b);
  grid.scale.z = depth / width;
  grid.position.y = 0.075;
  const gridMaterials = Array.isArray(grid.material) ? grid.material : [grid.material];
  for (const material of gridMaterials) {
    material.transparent = true;
    material.opacity = 0.14;
    material.depthWrite = false;
  }
  group.add(grid);

  const label = createTextSprite(domain.label, {
    color: '#edf7ef',
    fontSize: 43,
    fontWeight: 780,
    maxWidth: 920,
    scale: 0.003,
    uppercase: true,
  });
  label.position.set(-width / 2 + 2.55, 0.37, depth / 2 - 0.36);
  group.add(label);

  const shortLabel = createTextSprite(domain.shortLabel, {
    color: domain.color,
    fontSize: 27,
    fontWeight: 760,
    maxWidth: 520,
    scale: 0.0024,
    uppercase: true,
  });
  shortLabel.position.set(width / 2 - 1.85, 0.36, depth / 2 - 0.36);
  group.add(shortLabel);

  const description = createTextSprite(domain.description, {
    color: '#819488',
    fontSize: 28,
    fontWeight: 560,
    maxWidth: 1024,
    scale: 0.0025,
  });
  description.position.set(-width / 2 + 3.2, 0.28, -depth / 2 + 0.32);
  group.add(description);

  return { group, pad, border, grid, label, shortLabel, description };
}

function createMapCompass(): THREE.Group {
  const group = new THREE.Group();
  group.name = 'sandbox-compass';
  group.position.set(15.9, 0.04, 12.6);
  const ring = new THREE.Mesh(
    new THREE.RingGeometry(0.35, 0.39, 32),
    new THREE.MeshBasicMaterial({
      color: 0x91a398,
      transparent: true,
      opacity: 0.36,
      side: THREE.DoubleSide,
    })
  );
  ring.rotation.x = -Math.PI / 2;
  group.add(ring);
  const north = new THREE.Mesh(
    new THREE.ConeGeometry(0.12, 0.48, 3),
    new THREE.MeshBasicMaterial({ color: 0xb8f36b, transparent: true, opacity: 0.72 })
  );
  north.position.set(0, 0.03, -0.28);
  north.rotation.x = -Math.PI / 2;
  group.add(north);
  return group;
}

function createStars(): THREE.Points<THREE.BufferGeometry, THREE.PointsMaterial> {
  const random = deterministicRandom(0x0a35c10d);
  const positions = new Float32Array(520 * 3);
  for (let index = 0; index < positions.length; index += 3) {
    const radius = 22 + random() * 36;
    const theta = random() * Math.PI * 2;
    positions[index] = Math.cos(theta) * radius;
    positions[index + 1] = 5 + random() * 28;
    positions[index + 2] = Math.sin(theta) * radius;
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  return new THREE.Points(
    geometry,
    new THREE.PointsMaterial({
      color: 0x91ad9c,
      size: 0.055,
      transparent: true,
      opacity: 0.36,
      depthWrite: false,
      sizeAttenuation: true,
    })
  );
}

function deterministicRandom(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state / 0x1_0000_0000;
  };
}
