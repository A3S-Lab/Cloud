import { useEffect, useRef } from 'react';

type AmbientGridProps = {
  className?: string;
};

type Wave = {
  bornAt: number;
  strength: number;
  x: number;
  y: number;
};

const CELL_SIZE = 42;
const WAVE_LIFETIME = 1_800;

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(Math.max(value, minimum), maximum);
}

/**
 * A Canvas 2D adaptation of the cursor-reactive Canvas UI grid concept.
 * See website/THIRD_PARTY_NOTICES.md for attribution and license terms.
 */
export function AmbientGrid({ className }: AmbientGridProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    const context = canvas?.getContext('2d');
    if (!canvas || !host || !context) return undefined;

    const drawingCanvas = canvas;
    const drawingContext = context;
    const gridHost = host;
    const page = drawingCanvas.closest<HTMLElement>('.cloud-home') ?? gridHost;
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    let reducedMotion = motion.matches;
    let frame = 0;
    let visible = true;
    let pointerInside = false;
    let previousPointer = { x: -CELL_SIZE, y: -CELL_SIZE };
    let pointer = { x: -CELL_SIZE * 4, y: -CELL_SIZE * 4 };
    let waves: Wave[] = [];
    let width = 1;
    let height = 1;
    let pixelRatio = 1;

    function schedule() {
      if (!frame && visible) frame = window.requestAnimationFrame(draw);
    }

    function resize() {
      const bounds = gridHost.getBoundingClientRect();
      width = Math.max(bounds.width, 1);
      height = Math.max(bounds.height, 1);
      pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
      drawingCanvas.width = Math.round(width * pixelRatio);
      drawingCanvas.height = Math.round(height * pixelRatio);
      schedule();
    }

    function draw(now: number) {
      frame = 0;
      drawingContext.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
      drawingContext.clearRect(0, 0, width, height);

      const activeWaves = reducedMotion
        ? []
        : waves.filter((wave) => now - wave.bornAt < WAVE_LIFETIME);
      waves = activeWaves;
      const columns = Math.ceil(width / CELL_SIZE) + 1;
      const rows = Math.ceil(height / CELL_SIZE) + 1;
      const maximumRadius = Math.max(width, height) * 0.85;

      for (let row = 0; row < rows; row += 1) {
        for (let column = 0; column < columns; column += 1) {
          const centerX = column * CELL_SIZE + CELL_SIZE / 2;
          const centerY = row * CELL_SIZE + CELL_SIZE / 2;
          const pointerDistance = Math.hypot(
            centerX - pointer.x,
            centerY - pointer.y,
          );
          const pointerLift = pointerInside
            ? Math.exp(-pointerDistance / (CELL_SIZE * 2.4))
            : 0;
          let waveLift = 0;

          for (const wave of activeWaves) {
            const progress = clamp((now - wave.bornAt) / WAVE_LIFETIME, 0, 1);
            const radius = progress * maximumRadius;
            const distance = Math.hypot(centerX - wave.x, centerY - wave.y);
            const ring = Math.exp(
              -Math.pow((distance - radius) / (CELL_SIZE * 1.65), 2),
            );
            waveLift = Math.max(
              waveLift,
              ring * (1 - progress) * wave.strength,
            );
          }

          const lift = clamp(pointerLift * 0.62 + waveLift, 0, 1);
          const inset = 5 - lift * 2.4;
          const raised = -lift * 6;
          const x = column * CELL_SIZE + inset;
          const y = row * CELL_SIZE + inset + raised;
          const size = CELL_SIZE - inset * 2;
          const alpha = 0.035 + lift * 0.2;

          drawingContext.fillStyle = `rgba(116, 243, 189, ${0.004 + lift * 0.035})`;
          drawingContext.fillRect(x, y, size, size);
          drawingContext.strokeStyle = `rgba(123, 201, 170, ${alpha})`;
          drawingContext.lineWidth = 1;
          drawingContext.strokeRect(x, y, size, size);

          if (lift > 0.08) {
            drawingContext.fillStyle = `rgba(111, 178, 255, ${lift * 0.045})`;
            drawingContext.fillRect(x, y + size, size, lift * 6);
          }
        }
      }

      if (activeWaves.length > 0) schedule();
    }

    function addWave(x: number, y: number, strength: number) {
      waves = [
        ...waves.slice(-4),
        { bornAt: performance.now(), strength, x, y },
      ];
      schedule();
    }

    function handlePointerMove(event: PointerEvent) {
      if (reducedMotion || event.pointerType === 'touch') return;
      const bounds = gridHost.getBoundingClientRect();
      const x = event.clientX - bounds.left;
      const y = event.clientY - bounds.top;
      if (y < -CELL_SIZE || y > bounds.height + CELL_SIZE) return;

      pointer = { x, y };
      pointerInside = true;
      if (
        Math.hypot(x - previousPointer.x, y - previousPointer.y) >
        CELL_SIZE * 1.4
      ) {
        addWave(x, y, 0.72);
        previousPointer = { x, y };
      }
      schedule();
    }

    function handlePointerLeave() {
      pointerInside = false;
      pointer = { x: -CELL_SIZE * 4, y: -CELL_SIZE * 4 };
      schedule();
    }

    function handleMotionChange() {
      reducedMotion = motion.matches;
      if (reducedMotion) waves = [];
      schedule();
    }

    const resizeObserver =
      'ResizeObserver' in window ? new ResizeObserver(resize) : undefined;
    const intersectionObserver =
      'IntersectionObserver' in window
        ? new IntersectionObserver(([entry]) => {
            visible = entry?.isIntersecting ?? true;
            if (visible) schedule();
          })
        : undefined;
    resizeObserver?.observe(gridHost);
    intersectionObserver?.observe(gridHost);
    window.addEventListener('resize', resize);
    page.addEventListener('pointermove', handlePointerMove);
    page.addEventListener('pointerleave', handlePointerLeave);
    motion.addEventListener('change', handleMotionChange);
    resize();
    if (!reducedMotion) addWave(width * 0.68, height * 0.34, 0.68);

    const ambientTimer = window.setInterval(() => {
      if (!reducedMotion && visible) {
        addWave(width * (0.25 + Math.random() * 0.5), height * 0.42, 0.38);
      }
    }, 5_400);

    return () => {
      window.cancelAnimationFrame(frame);
      window.clearInterval(ambientTimer);
      resizeObserver?.disconnect();
      intersectionObserver?.disconnect();
      window.removeEventListener('resize', resize);
      page.removeEventListener('pointermove', handlePointerMove);
      page.removeEventListener('pointerleave', handlePointerLeave);
      motion.removeEventListener('change', handleMotionChange);
    };
  }, []);

  return <canvas aria-hidden="true" className={className} ref={canvasRef} />;
}
