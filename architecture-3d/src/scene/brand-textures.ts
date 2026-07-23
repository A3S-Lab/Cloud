import * as THREE from 'three';
import type { ArchitectureLogoId } from '../architecture';

type LogoGlyph =
  | 'a3s'
  | 'antenna'
  | 'api'
  | 'blocks'
  | 'box'
  | 'branch'
  | 'chip'
  | 'client'
  | 'containers'
  | 'docker'
  | 'event'
  | 'flow'
  | 'gateway'
  | 'github'
  | 'health'
  | 'neural'
  | 'oci'
  | 'package'
  | 'postgres'
  | 'radar'
  | 'route'
  | 'runtime'
  | 'shield'
  | 'storage'
  | 'timeline';

interface BrandSpec {
  label: string;
  eyebrow: string;
  glyph: LogoGlyph;
  accent: string;
}

const BRAND_SPECS: Readonly<Record<ArchitectureLogoId, BrandSpec>> = {
  clients: { label: 'Clients + SDKs', eyebrow: 'REQUEST ORIGIN', glyph: 'client', accent: '#72b7ff' },
  'a3s-web': { label: 'A3S Web', eyebrow: 'CONSOLE', glyph: 'a3s', accent: '#b8f36b' },
  'a3s-box': { label: 'A3S Box', eyebrow: 'LOCAL RUNTIME', glyph: 'box', accent: '#71d5c3' },
  'a3s-box-provider': {
    label: 'A3S Box',
    eyebrow: 'WORKLOAD PROVIDER',
    glyph: 'box',
    accent: '#71d5c3',
  },
  'a3s-code': { label: 'A3S Code', eyebrow: 'TUI · CLI · MCP', glyph: 'client', accent: '#b8f36b' },
  github: { label: 'GitHub', eyebrow: 'SOURCE', glyph: 'github', accent: '#f0f6fc' },
  inference: { label: 'A3S Inference', eyebrow: 'GPU PROFILE', glyph: 'neural', accent: '#d7b6ff' },
  'a3s-boot': { label: 'A3S Boot', eyebrow: 'API BOUNDARY', glyph: 'a3s', accent: '#b8f36b' },
  identity: { label: 'Identity', eyebrow: 'AUTHORITY', glyph: 'shield', accent: '#8fd6ff' },
  projects: { label: 'Projects', eyebrow: 'NAMESPACE', glyph: 'blocks', accent: '#b8f36b' },
  sources: { label: 'Sources', eyebrow: 'REVISIONS', glyph: 'branch', accent: '#72b7ff' },
  artifacts: { label: 'Artifacts', eyebrow: 'BUILD + RELEASE', glyph: 'package', accent: '#f3c86b' },
  workloads: { label: 'Workloads', eyebrow: 'DESIRED STATE', glyph: 'containers', accent: '#b8f36b' },
  fleet: { label: 'Fleet', eyebrow: 'NODE CONTROL', glyph: 'radar', accent: '#71d5c3' },
  edge: { label: 'Edge', eyebrow: 'ROUTE POLICY', glyph: 'route', accent: '#72b7ff' },
  operations: { label: 'Operations', eyebrow: 'DURABLE PROGRESS', glyph: 'timeline', accent: '#d7b6ff' },
  postgresql: { label: 'PostgreSQL', eyebrow: 'SOURCE OF TRUTH', glyph: 'postgres', accent: '#5f9fd7' },
  'a3s-flow': { label: 'A3S Flow', eyebrow: 'WORKFLOWS', glyph: 'flow', accent: '#d7b6ff' },
  'a3s-event': { label: 'A3S Event', eyebrow: 'COMMITTED FACTS', glyph: 'event', accent: '#ef9cff' },
  'object-store': { label: 'Object Store', eyebrow: 'CONTENT BYTES', glyph: 'storage', accent: '#f3c86b' },
  'node-agent': { label: 'Node Agent', eyebrow: 'OUTBOUND MTLS', glyph: 'antenna', accent: '#71d5c3' },
  'a3s-runtime': { label: 'A3S Runtime', eyebrow: 'EXECUTION', glyph: 'runtime', accent: '#71d5c3' },
  'a3s-gateway': { label: 'A3S Gateway', eyebrow: 'TRAFFIC', glyph: 'gateway', accent: '#72b7ff' },
  'docker-buildkit': {
    label: 'Docker + BuildKit',
    eyebrow: 'BUILD PROVIDER',
    glyph: 'docker',
    accent: '#2496ed',
  },
  'runtime-unit': { label: 'Runtime Unit', eyebrow: 'HEALTHY TARGET', glyph: 'health', accent: '#b8f36b' },
  'oci-registry': { label: 'OCI Registry', eyebrow: 'DIGEST ONLY', glyph: 'oci', accent: '#f7941d' },
  'cpu-compute': { label: 'CPU Compute', eyebrow: 'GENERAL PURPOSE', glyph: 'chip', accent: '#69b7ff' },
  'gpu-compute': { label: 'GPU Compute', eyebrow: 'ACCELERATOR', glyph: 'neural', accent: '#9b8cff' },
};

export function createBrandSprite(logoId: ArchitectureLogoId): THREE.Sprite {
  const canvas = document.createElement('canvas');
  canvas.width = 640;
  canvas.height = 184;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('A3S architecture brand badges require a 2D canvas context');

  const spec = BRAND_SPECS[logoId];
  drawBadge(context, canvas, spec);

  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.minFilter = THREE.LinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.generateMipmaps = false;

  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({
      map: texture,
      color: 0xffffff,
      depthTest: false,
      depthWrite: false,
      transparent: true,
    })
  );
  sprite.name = `logo:${logoId}`;
  sprite.scale.set(2.56, 0.736, 1);
  sprite.renderOrder = 12;
  return sprite;
}

export function disposeBrandSprite(sprite: THREE.Sprite): void {
  sprite.material.map?.dispose();
  sprite.material.dispose();
}

function drawBadge(context: CanvasRenderingContext2D, canvas: HTMLCanvasElement, spec: BrandSpec): void {
  context.clearRect(0, 0, canvas.width, canvas.height);
  roundedRect(context, 3, 3, canvas.width - 6, canvas.height - 6, 30);
  const background = context.createLinearGradient(0, 0, canvas.width, canvas.height);
  background.addColorStop(0, 'rgba(8, 16, 12, 0.97)');
  background.addColorStop(1, 'rgba(15, 26, 19, 0.94)');
  context.fillStyle = background;
  context.fill();
  context.strokeStyle = withAlpha(spec.accent, 0.62);
  context.lineWidth = 3;
  context.stroke();

  context.beginPath();
  context.arc(91, 92, 57, 0, Math.PI * 2);
  context.fillStyle = withAlpha(spec.accent, 0.12);
  context.fill();
  context.strokeStyle = withAlpha(spec.accent, 0.42);
  context.lineWidth = 2;
  context.stroke();
  drawGlyph(context, spec.glyph, spec.accent, 91, 92);

  context.fillStyle = '#edf6ef';
  context.font = '700 37px ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
  context.textAlign = 'left';
  context.textBaseline = 'middle';
  context.fillText(spec.label, 172, 81, 430);

  context.fillStyle = spec.accent;
  context.font = '700 18px ui-monospace, "SFMono-Regular", Consolas, monospace';
  context.letterSpacing = '2px';
  context.fillText(spec.eyebrow, 173, 124, 425);
}

function drawGlyph(
  context: CanvasRenderingContext2D,
  glyph: LogoGlyph,
  color: string,
  x: number,
  y: number
): void {
  context.save();
  context.translate(x, y);
  context.strokeStyle = color;
  context.fillStyle = color;
  context.lineCap = 'round';
  context.lineJoin = 'round';
  context.lineWidth = 7;

  switch (glyph) {
    case 'a3s':
      line(context, -28, -21, 28, -21);
      line(context, -20, 0, 20, 0);
      line(context, -11, 21, 11, 21);
      break;
    case 'client':
      context.strokeRect(-31, -25, 62, 43);
      line(context, -12, 29, 12, 29);
      line(context, 0, 18, 0, 29);
      context.font = '700 21px ui-monospace, monospace';
      context.textAlign = 'center';
      context.fillText('>_', 0, 4);
      break;
    case 'chip':
      context.strokeRect(-25, -25, 50, 50);
      context.strokeRect(-13, -13, 26, 26);
      for (const offset of [-19, -6, 7, 20]) {
        line(context, offset, -34, offset, -25, 3);
        line(context, offset, 25, offset, 34, 3);
        line(context, -34, offset, -25, offset, 3);
        line(context, 25, offset, 34, offset, 3);
      }
      break;
    case 'api':
      circle(context, -25, 0, 8);
      circle(context, 25, -21, 8);
      circle(context, 25, 21, 8);
      line(context, -16, -2, 15, -17);
      line(context, -16, 2, 15, 17);
      break;
    case 'github':
      context.beginPath();
      context.moveTo(-29, -8);
      context.lineTo(-24, -31);
      context.lineTo(-7, -22);
      context.quadraticCurveTo(0, -25, 7, -22);
      context.lineTo(24, -31);
      context.lineTo(29, -8);
      context.arc(0, 2, 30, 0, Math.PI, false);
      context.quadraticCurveTo(18, 31, 0, 31);
      context.quadraticCurveTo(-18, 31, -29, -8);
      context.fill();
      context.fillStyle = '#07100d';
      circle(context, -11, 2, 4);
      circle(context, 11, 2, 4);
      break;
    case 'neural':
      for (const [fromX, fromY, toX, toY] of [
        [-27, -21, 0, 0],
        [-27, 21, 0, 0],
        [0, 0, 27, -24],
        [0, 0, 30, 17],
        [-27, -21, 27, -24],
      ]) {
        line(context, fromX, fromY, toX, toY, 4);
      }
      for (const [dotX, dotY] of [
        [-27, -21],
        [-27, 21],
        [0, 0],
        [27, -24],
        [30, 17],
      ]) {
        circle(context, dotX, dotY, 7);
      }
      break;
    case 'shield':
      context.beginPath();
      context.moveTo(0, -34);
      context.lineTo(28, -23);
      context.lineTo(23, 10);
      context.quadraticCurveTo(17, 28, 0, 36);
      context.quadraticCurveTo(-17, 28, -23, 10);
      context.lineTo(-28, -23);
      context.closePath();
      context.stroke();
      line(context, -11, 1, -2, 12);
      line(context, -2, 12, 15, -10);
      break;
    case 'blocks':
      context.strokeRect(-30, 0, 25, 25);
      context.strokeRect(5, 0, 25, 25);
      context.strokeRect(-12, -31, 25, 25);
      break;
    case 'box':
      context.beginPath();
      context.moveTo(0, -34);
      context.lineTo(31, -17);
      context.lineTo(31, 19);
      context.lineTo(0, 36);
      context.lineTo(-31, 19);
      context.lineTo(-31, -17);
      context.closePath();
      context.stroke();
      line(context, 0, 0, 0, 35, 4);
      line(context, 0, 0, 30, -17, 4);
      line(context, 0, 0, -30, -17, 4);
      break;
    case 'branch':
      line(context, -19, -29, -19, 27);
      line(context, -19, -7, 18, -7);
      line(context, 18, -7, 18, 22);
      circle(context, -19, -30, 7);
      circle(context, -19, 29, 7);
      circle(context, 18, 24, 7);
      break;
    case 'package':
      context.strokeRect(-27, -23, 54, 50);
      line(context, -27, -23, 0, -6);
      line(context, 27, -23, 0, -6);
      line(context, 0, -6, 0, 27);
      break;
    case 'containers':
      context.strokeRect(-31, -24, 28, 48);
      context.strokeRect(4, -24, 28, 48);
      line(context, -22, -14, -22, 14, 3);
      line(context, -13, -14, -13, 14, 3);
      line(context, 13, -14, 13, 14, 3);
      line(context, 22, -14, 22, 14, 3);
      break;
    case 'radar':
      context.beginPath();
      context.arc(0, 5, 30, Math.PI, Math.PI * 2);
      context.stroke();
      context.beginPath();
      context.arc(0, 5, 18, Math.PI * 1.18, Math.PI * 1.82);
      context.stroke();
      line(context, 0, 5, 24, -16);
      circle(context, 0, 5, 5);
      line(context, -12, 34, 12, 34);
      break;
    case 'route':
      line(context, -31, 19, -11, 19);
      context.beginPath();
      context.quadraticCurveTo(-4, 19, -4, 6);
      context.quadraticCurveTo(-4, -19, 16, -19);
      context.stroke();
      line(context, 16, -19, 29, -19);
      line(context, 18, -30, 30, -19);
      line(context, 30, -19, 18, -8);
      break;
    case 'timeline':
      line(context, -31, 0, 31, 0);
      for (const markerX of [-27, 0, 27]) circle(context, markerX, 0, 8);
      line(context, -27, -16, -27, -28, 3);
      line(context, 0, 16, 0, 28, 3);
      line(context, 27, -16, 27, -28, 3);
      break;
    case 'postgres':
      context.beginPath();
      context.ellipse(0, -6, 24, 30, 0, 0, Math.PI * 2);
      context.stroke();
      context.beginPath();
      context.ellipse(-26, -8, 12, 19, -0.35, 0, Math.PI * 2);
      context.stroke();
      context.beginPath();
      context.ellipse(26, -8, 12, 19, 0.35, 0, Math.PI * 2);
      context.stroke();
      context.beginPath();
      context.moveTo(8, 10);
      context.quadraticCurveTo(10, 35, -7, 35);
      context.stroke();
      break;
    case 'flow':
      context.beginPath();
      context.arc(0, 0, 28, 0.35, Math.PI * 1.45);
      context.stroke();
      line(context, -28, -17, -29, 2);
      line(context, -29, 2, -11, -2);
      context.beginPath();
      context.arc(0, 0, 17, Math.PI * 1.45, 0.35);
      context.stroke();
      break;
    case 'event':
      line(context, -31, 0, 31, 0);
      for (const markerX of [-23, -8, 8, 23]) {
        const height = markerX % 2 === 0 ? 21 : 29;
        line(context, markerX, -height, markerX, height, 4);
      }
      break;
    case 'storage':
      context.beginPath();
      context.ellipse(0, -22, 29, 12, 0, 0, Math.PI * 2);
      context.stroke();
      line(context, -29, -22, -29, 23);
      line(context, 29, -22, 29, 23);
      context.beginPath();
      context.ellipse(0, 23, 29, 12, 0, 0, Math.PI);
      context.stroke();
      context.beginPath();
      context.ellipse(0, 0, 29, 12, 0, 0, Math.PI);
      context.stroke();
      break;
    case 'antenna':
      line(context, 0, -28, 0, 31);
      line(context, -16, 31, 16, 31);
      circle(context, 0, -29, 6);
      context.beginPath();
      context.arc(0, -29, 19, -0.8, 0.8);
      context.stroke();
      context.beginPath();
      context.arc(0, -29, 31, -0.75, 0.75);
      context.stroke();
      break;
    case 'runtime':
      circle(context, 0, 0, 18, false);
      for (let index = 0; index < 8; index += 1) {
        const angle = (index / 8) * Math.PI * 2;
        line(context, Math.cos(angle) * 22, Math.sin(angle) * 22, Math.cos(angle) * 34, Math.sin(angle) * 34);
      }
      circle(context, 0, 0, 6);
      break;
    case 'gateway':
      context.beginPath();
      context.moveTo(-30, 30);
      context.lineTo(-30, -7);
      context.quadraticCurveTo(-30, -30, 0, -30);
      context.quadraticCurveTo(30, -30, 30, -7);
      context.lineTo(30, 30);
      context.stroke();
      line(context, -18, 8, 18, 8);
      line(context, 8, -2, 18, 8);
      line(context, 18, 8, 8, 18);
      break;
    case 'docker':
      for (const [boxX, boxY] of [
        [-24, -15],
        [-8, -15],
        [8, -15],
        [-8, -30],
      ]) {
        context.strokeRect(boxX, boxY, 13, 13);
      }
      context.beginPath();
      context.moveTo(-33, 2);
      context.lineTo(24, 2);
      context.quadraticCurveTo(20, 27, -8, 28);
      context.quadraticCurveTo(-29, 27, -33, 2);
      context.stroke();
      line(context, 24, 3, 34, -6);
      break;
    case 'health':
      circle(context, 0, 0, 32, false);
      line(context, -17, 1, -4, 15);
      line(context, -4, 15, 20, -15);
      break;
    case 'oci':
      context.beginPath();
      for (let index = 0; index < 6; index += 1) {
        const angle = -Math.PI / 2 + (index / 6) * Math.PI * 2;
        const pointX = Math.cos(angle) * 34;
        const pointY = Math.sin(angle) * 34;
        if (index === 0) context.moveTo(pointX, pointY);
        else context.lineTo(pointX, pointY);
      }
      context.closePath();
      context.stroke();
      context.font = '800 18px ui-sans-serif, sans-serif';
      context.textAlign = 'center';
      context.textBaseline = 'middle';
      context.fillText('OCI', 0, 1);
      break;
  }
  context.restore();
}

function line(
  context: CanvasRenderingContext2D,
  fromX: number,
  fromY: number,
  toX: number,
  toY: number,
  width = 7
): void {
  context.beginPath();
  context.lineWidth = width;
  context.moveTo(fromX, fromY);
  context.lineTo(toX, toY);
  context.stroke();
}

function circle(context: CanvasRenderingContext2D, x: number, y: number, radius: number, fill = true): void {
  context.beginPath();
  context.arc(x, y, radius, 0, Math.PI * 2);
  if (fill) context.fill();
  else context.stroke();
}

function roundedRect(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number
): void {
  context.beginPath();
  context.moveTo(x + radius, y);
  context.lineTo(x + width - radius, y);
  context.quadraticCurveTo(x + width, y, x + width, y + radius);
  context.lineTo(x + width, y + height - radius);
  context.quadraticCurveTo(x + width, y + height, x + width - radius, y + height);
  context.lineTo(x + radius, y + height);
  context.quadraticCurveTo(x, y + height, x, y + height - radius);
  context.lineTo(x, y + radius);
  context.quadraticCurveTo(x, y, x + radius, y);
  context.closePath();
}

function withAlpha(color: string, alpha: number): string {
  const value = color.replace('#', '');
  const red = Number.parseInt(value.slice(0, 2), 16);
  const green = Number.parseInt(value.slice(2, 4), 16);
  const blue = Number.parseInt(value.slice(4, 6), 16);
  return `rgba(${red}, ${green}, ${blue}, ${alpha})`;
}
