import * as THREE from 'three';

interface TextSpriteOptions {
  color?: THREE.ColorRepresentation;
  fontSize?: number;
  fontWeight?: number;
  maxWidth?: number;
  opacity?: number;
  padding?: number;
  scale?: number;
  uppercase?: boolean;
}

export function createTextSprite(text: string, options: TextSpriteOptions = {}): THREE.Sprite {
  const {
    color = '#e9f1eb',
    fontSize = 42,
    fontWeight = 650,
    maxWidth = 820,
    opacity = 1,
    padding = 32,
    scale = 0.0045,
    uppercase = false,
  } = options;
  const canvas = document.createElement('canvas');
  const context = canvas.getContext('2d');
  if (!context) throw new Error('A3S architecture labels require a 2D canvas context');

  const value = uppercase ? text.toUpperCase() : text;
  context.font = `${fontWeight} ${fontSize}px ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
  const measuredWidth = Math.ceil(context.measureText(value).width);
  canvas.width = Math.min(maxWidth, Math.max(128, measuredWidth + padding * 2));
  canvas.height = fontSize + padding * 2;

  context.clearRect(0, 0, canvas.width, canvas.height);
  context.font = `${fontWeight} ${fontSize}px ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
  context.fillStyle = new THREE.Color(color).getStyle();
  context.textAlign = 'center';
  context.textBaseline = 'middle';
  context.fillText(value, canvas.width / 2, canvas.height / 2, canvas.width - padding * 2);

  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.minFilter = THREE.LinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.generateMipmaps = false;

  const material = new THREE.SpriteMaterial({
    map: texture,
    color: 0xffffff,
    depthTest: false,
    opacity,
    transparent: true,
    depthWrite: false,
  });
  const sprite = new THREE.Sprite(material);
  sprite.scale.set(canvas.width * scale, canvas.height * scale, 1);
  sprite.renderOrder = 8;
  return sprite;
}

export function disposeTextSprite(sprite: THREE.Sprite): void {
  const material = sprite.material;
  material.map?.dispose();
  material.dispose();
}
