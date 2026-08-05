import { useEffect, useRef } from 'react';

const SURFACE_SELECTOR = [
  '.cloud-hero-scene',
  '.cloud-editorial-chart',
  '.cloud-product-capabilities li',
  '.cloud-web-client-capabilities article',
  '.cloud-web-window',
  '.cloud-edge-web-capabilities article',
  '.cloud-industry-grid article',
  '.cloud-architecture-layer article',
].join(',');

const MOTION_SELECTOR = '.cloud-motion-scene';

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
    const visibleMotionItems = new Set<HTMLElement>();

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

    const motionItems = [
      ...pageHost.querySelectorAll<HTMLElement>(MOTION_SELECTOR),
    ];

    function syncMotionState() {
      const shouldRun = !motion.matches && !document.hidden;
      for (const item of motionItems) {
        item.classList.toggle(
          'is-motion-active',
          shouldRun && visibleMotionItems.has(item),
        );
      }
    }

    const motionObserver =
      'IntersectionObserver' in window
        ? new IntersectionObserver(
            (entries) => {
              for (const entry of entries) {
                const item = entry.target as HTMLElement;
                if (entry.isIntersecting) visibleMotionItems.add(item);
                else visibleMotionItems.delete(item);
              }
              syncMotionState();
            },
            { rootMargin: '120px 0px', threshold: 0.05 },
          )
        : undefined;

    for (const item of motionItems) {
      if (motionObserver) motionObserver.observe(item);
      else visibleMotionItems.add(item);
    }

    function handleMotionPreference() {
      if (motion.matches) clearSurface();
      syncMotionState();
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
    document.addEventListener('visibilitychange', syncMotionState);
    motion.addEventListener('change', handleMotionPreference);
    syncMotionState();

    return () => {
      window.cancelAnimationFrame(frame);
      revealObserver?.disconnect();
      motionObserver?.disconnect();
      clearSurface();
      for (const item of motionItems) item.classList.remove('is-motion-active');
      delete pageHost.dataset.effects;
      pageHost.removeEventListener('pointermove', handlePointerMove);
      pageHost.removeEventListener('pointerleave', handlePointerLeave);
      document.removeEventListener('visibilitychange', syncMotionState);
      motion.removeEventListener('change', handleMotionPreference);
    };
  }, []);

  return (
    <span aria-hidden="true" className="cloud-effects-anchor" ref={anchorRef} />
  );
}
