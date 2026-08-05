import { useEffect, useRef } from 'react';

const SURFACE_SELECTOR = [
  '.cloud-editorial-chart',
  '.cloud-product-capabilities li',
  '.cloud-web-client-capabilities article',
  '.cloud-web-window',
  '.cloud-edge-web-capabilities article',
  '.cloud-industry-grid article',
  '.cloud-architecture-layer article',
].join(',');

export function InteractionLayer() {
  const anchorRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const host = anchorRef.current?.closest<HTMLElement>('.cloud-home');
    if (!host) return undefined;

    const pageHost = host;
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    let activeSurface: HTMLElement | undefined;
    let frame = 0;
    let latestEvent: PointerEvent | undefined;

    function clearSurface() {
      activeSurface?.classList.remove('is-pointer-active');
      activeSurface = undefined;
    }

    function paint() {
      frame = 0;
      const event = latestEvent;
      if (!event || motion.matches) {
        clearSurface();
        return;
      }

      const target = event.target instanceof Element ? event.target : undefined;
      const surface = target?.closest<HTMLElement>(SURFACE_SELECTOR);
      if (!surface || !pageHost.contains(surface)) {
        clearSurface();
        return;
      }

      if (surface !== activeSurface) {
        clearSurface();
        activeSurface = surface;
        surface.classList.add('is-pointer-active');
      }
      const bounds = surface.getBoundingClientRect();
      surface.style.setProperty('--spot-x', `${event.clientX - bounds.left}px`);
      surface.style.setProperty('--spot-y', `${event.clientY - bounds.top}px`);
    }

    function handlePointerMove(event: PointerEvent) {
      if (event.pointerType === 'touch') return;
      latestEvent = event;
      if (!frame) frame = window.requestAnimationFrame(paint);
    }

    function handlePointerLeave() {
      latestEvent = undefined;
      clearSurface();
    }

    const revealItems = [
      ...pageHost.querySelectorAll<HTMLElement>('[data-reveal]'),
    ];
    const revealObserver =
      !motion.matches && 'IntersectionObserver' in window
        ? new IntersectionObserver(
            (entries) => {
              for (const entry of entries) {
                if (!entry.isIntersecting) continue;
                (entry.target as HTMLElement).classList.add('is-visible');
                revealObserver?.unobserve(entry.target);
              }
            },
            { rootMargin: '0px 0px -8% 0px', threshold: 0.08 },
          )
        : undefined;

    pageHost.dataset.effects = 'ready';
    for (const item of revealItems) {
      if (revealObserver) revealObserver.observe(item);
      else item.classList.add('is-visible');
    }
    pageHost.addEventListener('pointermove', handlePointerMove);
    pageHost.addEventListener('pointerleave', handlePointerLeave);

    return () => {
      window.cancelAnimationFrame(frame);
      revealObserver?.disconnect();
      clearSurface();
      delete pageHost.dataset.effects;
      pageHost.removeEventListener('pointermove', handlePointerMove);
      pageHost.removeEventListener('pointerleave', handlePointerLeave);
    };
  }, []);

  return (
    <span aria-hidden="true" className="cloud-effects-anchor" ref={anchorRef} />
  );
}
