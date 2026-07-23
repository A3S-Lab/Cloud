import * as THREE from 'three';
import { type ArchitectureSelection, isArchitectureSelection } from '../selection';

export interface ArchitectureHoverEvent {
  selection: ArchitectureSelection;
  x: number;
  y: number;
  placement: 'left' | 'right';
}

interface ArchitectureInteractionOptions {
  canvas: HTMLCanvasElement;
  camera: THREE.Camera;
  targets: readonly THREE.Object3D[];
  onHover: (event?: ArchitectureHoverEvent) => void;
  onSelect: (selection: ArchitectureSelection) => void;
  onReset: () => void;
}

export interface ArchitectureInteractionController {
  dispose: () => void;
}

export function attachArchitectureInteraction({
  canvas,
  camera,
  targets,
  onHover,
  onSelect,
  onReset,
}: ArchitectureInteractionOptions): ArchitectureInteractionController {
  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();
  let pointerStart:
    | {
        id: number;
        x: number;
        y: number;
      }
    | undefined;

  canvas.tabIndex = 0;
  canvas.setAttribute('role', 'application');
  canvas.setAttribute(
    'aria-label',
    'Interactive 3D map of A3S Cloud. Drag to orbit, scroll to zoom, select components or relationships for details, and press R to reset the camera.'
  );

  const hitTest = (event: PointerEvent): ArchitectureSelection | undefined => {
    const rect = canvas.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return undefined;
    pointer.set(
      ((event.clientX - rect.left) / rect.width) * 2 - 1,
      -((event.clientY - rect.top) / rect.height) * 2 + 1
    );
    raycaster.setFromCamera(pointer, camera);
    const selections: ArchitectureSelection[] = [];
    for (const intersection of raycaster.intersectObjects([...targets], false)) {
      const selection = intersection.object.userData.architectureSelection;
      if (isArchitectureSelection(selection)) {
        selections.push(selection);
        continue;
      }
      const nodeId = intersection.object.userData.nodeId;
      if (typeof nodeId === 'string') selections.push({ kind: 'node', id: nodeId });
    }
    return selections.find((selection) => selection.kind === 'node') ?? selections[0];
  };

  const handlePointerDown = (event: PointerEvent) => {
    pointerStart = { id: event.pointerId, x: event.clientX, y: event.clientY };
    canvas.focus({ preventScroll: true });
  };

  const handlePointerMove = (event: PointerEvent) => {
    if (
      pointerStart?.id === event.pointerId &&
      Math.hypot(event.clientX - pointerStart.x, event.clientY - pointerStart.y) > 5
    ) {
      canvas.dataset.interaction = 'orbit';
      onHover(undefined);
      return;
    }
    const selection = hitTest(event);
    canvas.dataset.interaction = selection ? (selection.kind === 'node' ? 'node' : 'relationship') : 'orbit';
    if (!selection) {
      onHover(undefined);
      return;
    }
    const rect = canvas.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    onHover({
      selection,
      x,
      y,
      placement: x > rect.width * 0.64 ? 'left' : 'right',
    });
  };

  const handlePointerUp = (event: PointerEvent) => {
    const start = pointerStart;
    pointerStart = undefined;
    if (
      !start ||
      start.id !== event.pointerId ||
      Math.hypot(event.clientX - start.x, event.clientY - start.y) > 5
    ) {
      return;
    }
    const selection = hitTest(event);
    if (selection) onSelect(selection);
  };

  const handlePointerCancel = () => {
    pointerStart = undefined;
  };

  const handlePointerLeave = () => {
    pointerStart = undefined;
    canvas.dataset.interaction = 'orbit';
    onHover(undefined);
  };

  const handleKeyDown = (event: KeyboardEvent) => {
    if (event.key.toLocaleLowerCase() !== 'r') return;
    event.preventDefault();
    onReset();
  };

  canvas.addEventListener('pointerdown', handlePointerDown);
  canvas.addEventListener('pointermove', handlePointerMove);
  canvas.addEventListener('pointerup', handlePointerUp);
  canvas.addEventListener('pointercancel', handlePointerCancel);
  canvas.addEventListener('pointerleave', handlePointerLeave);
  canvas.addEventListener('keydown', handleKeyDown);
  canvas.dataset.interaction = 'orbit';

  return {
    dispose: () => {
      canvas.removeEventListener('pointerdown', handlePointerDown);
      canvas.removeEventListener('pointermove', handlePointerMove);
      canvas.removeEventListener('pointerup', handlePointerUp);
      canvas.removeEventListener('pointercancel', handlePointerCancel);
      canvas.removeEventListener('pointerleave', handlePointerLeave);
      canvas.removeEventListener('keydown', handleKeyDown);
      onHover(undefined);
      delete canvas.dataset.interaction;
    },
  };
}
