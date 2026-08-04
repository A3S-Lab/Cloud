import { useEffect, useRef } from 'react';

type AmbientGridProps = {
  className?: string;
};

const CELL_SIZE = 42;
const CELL_INSET = 5;

/**
 * A static Canvas 2D adaptation of the Canvas UI grid concept.
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
    let frame = 0;

    function draw() {
      frame = 0;

      const bounds = gridHost.getBoundingClientRect();
      const pixelRatio = Math.min(window.devicePixelRatio || 1, 2);
      const width = Math.max(Math.round(bounds.width * pixelRatio), 1);
      const height = Math.max(Math.round(bounds.height * pixelRatio), 1);

      drawingCanvas.width = width;
      drawingCanvas.height = height;

      const tile = document.createElement('canvas');
      const tileSize = Math.round(CELL_SIZE * pixelRatio);
      const inset = CELL_INSET * pixelRatio;
      tile.width = tileSize;
      tile.height = tileSize;

      const tileContext = tile.getContext('2d');
      if (!tileContext) return;

      tileContext.fillStyle = 'rgba(116, 243, 189, 0.004)';
      tileContext.fillRect(
        inset,
        inset,
        tileSize - inset * 2,
        tileSize - inset * 2,
      );
      tileContext.strokeStyle = 'rgba(123, 201, 170, 0.035)';
      tileContext.lineWidth = pixelRatio;
      tileContext.strokeRect(
        inset,
        inset,
        tileSize - inset * 2,
        tileSize - inset * 2,
      );

      const pattern = drawingContext.createPattern(tile, 'repeat');
      drawingContext.clearRect(0, 0, width, height);
      if (!pattern) return;

      drawingContext.fillStyle = pattern;
      drawingContext.fillRect(0, 0, width, height);
    }

    function scheduleDraw() {
      if (!frame) frame = window.requestAnimationFrame(draw);
    }

    const resizeObserver =
      'ResizeObserver' in window ? new ResizeObserver(scheduleDraw) : undefined;

    resizeObserver?.observe(gridHost);
    if (!resizeObserver) window.addEventListener('resize', scheduleDraw);
    scheduleDraw();

    return () => {
      window.cancelAnimationFrame(frame);
      resizeObserver?.disconnect();
      if (!resizeObserver) window.removeEventListener('resize', scheduleDraw);
    };
  }, []);

  return <canvas aria-hidden="true" className={className} ref={canvasRef} />;
}
