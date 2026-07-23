import * as THREE from 'three';
import type { ArchitectureLayer } from '../architecture';
import { createTextSprite, disposeTextSprite } from './text-sprite';

interface LayerPlatform {
  group: THREE.Group;
  plane: THREE.Mesh<THREE.PlaneGeometry, THREE.MeshPhysicalMaterial>;
  grid: THREE.GridHelper;
  label: THREE.Sprite;
  description: THREE.Sprite;
}

export interface ArchitectureEnvironment {
  group: THREE.Group;
  platforms: readonly LayerPlatform[];
  stars: THREE.Points<THREE.BufferGeometry, THREE.PointsMaterial>;
  spine: THREE.LineSegments<THREE.BufferGeometry, THREE.LineBasicMaterial>;
  lights: readonly THREE.Light[];
}

export function createArchitectureEnvironment(
  scene: THREE.Scene,
  layers: readonly ArchitectureLayer[]
): ArchitectureEnvironment {
  const group = new THREE.Group();
  group.name = 'architecture-environment';
  const platforms = layers.map(createLayerPlatform);
  for (const platform of platforms) group.add(platform.group);

  const stars = createStars();
  group.add(stars);
  const spine = createSpine(layers);
  group.add(spine);

  const hemisphere = new THREE.HemisphereLight(0xb8f36b, 0x07100b, 1.7);
  const key = new THREE.DirectionalLight(0xd9ff9f, 2.3);
  key.position.set(-8, 15, 12);
  key.castShadow = true;
  key.shadow.mapSize.set(2048, 2048);
  key.shadow.camera.left = -18;
  key.shadow.camera.right = 18;
  key.shadow.camera.top = 16;
  key.shadow.camera.bottom = -16;
  const rim = new THREE.PointLight(0x72b7ff, 24, 38, 2);
  rim.position.set(10, 5, -8);
  const fill = new THREE.PointLight(0xd7b6ff, 18, 34, 2);
  fill.position.set(-11, -4, 8);
  const lights: readonly THREE.Light[] = [hemisphere, key, rim, fill];
  scene.add(group, ...lights);
  return { group, platforms, stars, spine, lights };
}

export function updateArchitectureEnvironment(
  environment: ArchitectureEnvironment,
  elapsed: number,
  reducedMotion: boolean
): void {
  if (reducedMotion) return;
  environment.stars.rotation.y = elapsed * 0.004;
  environment.stars.rotation.x = Math.sin(elapsed * 0.03) * 0.025;
  for (const [index, platform] of environment.platforms.entries()) {
    platform.plane.material.opacity = 0.058 + Math.sin(elapsed * 0.28 + index) * 0.008;
  }
}

export function disposeArchitectureEnvironment(
  scene: THREE.Scene,
  environment: ArchitectureEnvironment
): void {
  scene.remove(environment.group, ...environment.lights);
  for (const platform of environment.platforms) {
    platform.plane.geometry.dispose();
    platform.plane.material.dispose();
    platform.grid.geometry.dispose();
    const materials = Array.isArray(platform.grid.material)
      ? platform.grid.material
      : [platform.grid.material];
    for (const material of materials) material.dispose();
    disposeTextSprite(platform.label);
    disposeTextSprite(platform.description);
  }
  environment.stars.geometry.dispose();
  environment.stars.material.dispose();
  environment.spine.geometry.dispose();
  environment.spine.material.dispose();
}

function createLayerPlatform(layer: ArchitectureLayer): LayerPlatform {
  const group = new THREE.Group();
  group.name = `layer:${layer.id}`;
  group.position.y = layer.y;

  const plane = new THREE.Mesh(
    new THREE.PlaneGeometry(22, 7.4),
    new THREE.MeshPhysicalMaterial({
      color: new THREE.Color(layer.color).multiplyScalar(0.2),
      emissive: layer.color,
      emissiveIntensity: 0.035,
      metalness: 0.18,
      roughness: 0.76,
      transparent: true,
      opacity: 0.06,
      depthWrite: false,
      side: THREE.DoubleSide,
    })
  );
  plane.rotation.x = -Math.PI / 2;
  plane.receiveShadow = true;
  group.add(plane);

  const grid = new THREE.GridHelper(22, 22, layer.color, 0x213128);
  grid.scale.z = 0.34;
  grid.position.y = 0.025;
  const gridMaterials = Array.isArray(grid.material) ? grid.material : [grid.material];
  for (const material of gridMaterials) {
    material.transparent = true;
    material.opacity = 0.18;
    material.depthWrite = false;
  }
  group.add(grid);

  const label = createTextSprite(layer.label, {
    color: layer.color,
    fontSize: 39,
    fontWeight: 780,
    maxWidth: 640,
    scale: 0.0036,
    uppercase: true,
  });
  label.position.set(-8.8, 0.16, -3.1);
  group.add(label);

  const description = createTextSprite(layer.description, {
    color: '#7f9387',
    fontSize: 30,
    fontWeight: 540,
    maxWidth: 920,
    scale: 0.0028,
  });
  description.position.set(-6.8, -0.13, -3.1);
  group.add(description);

  return { group, plane, grid, label, description };
}

function createStars(): THREE.Points<THREE.BufferGeometry, THREE.PointsMaterial> {
  const random = deterministicRandom(0x0a35c10d);
  const positions = new Float32Array(760 * 3);
  for (let index = 0; index < positions.length; index += 3) {
    const radius = 18 + random() * 38;
    const theta = random() * Math.PI * 2;
    const height = -14 + random() * 32;
    positions[index] = Math.cos(theta) * radius;
    positions[index + 1] = height;
    positions[index + 2] = Math.sin(theta) * radius;
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  return new THREE.Points(
    geometry,
    new THREE.PointsMaterial({
      color: 0x8fafa0,
      size: 0.045,
      transparent: true,
      opacity: 0.42,
      depthWrite: false,
      sizeAttenuation: true,
    })
  );
}

function createSpine(
  layers: readonly ArchitectureLayer[]
): THREE.LineSegments<THREE.BufferGeometry, THREE.LineBasicMaterial> {
  const top = Math.max(...layers.map((layer) => layer.y)) + 0.2;
  const bottom = Math.min(...layers.map((layer) => layer.y)) - 0.2;
  const positions = new Float32Array([
    -10.6,
    top,
    -3.4,
    -10.6,
    bottom,
    -3.4,
    10.6,
    top,
    -3.4,
    10.6,
    bottom,
    -3.4,
  ]);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  return new THREE.LineSegments(
    geometry,
    new THREE.LineBasicMaterial({
      color: 0x52685b,
      transparent: true,
      opacity: 0.28,
      depthWrite: false,
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
