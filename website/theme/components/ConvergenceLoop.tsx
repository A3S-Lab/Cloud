import { useEffect, useRef, useState } from 'react';
import { convergenceSteps } from '../data/product';

function useReducedMotion() {
  const [reduced, setReduced] = useState(false);

  useEffect(() => {
    const preference = window.matchMedia('(prefers-reduced-motion: reduce)');
    const update = () => setReduced(preference.matches);
    update();
    preference.addEventListener('change', update);
    return () => preference.removeEventListener('change', update);
  }, []);

  return reduced;
}

export function ConvergenceLoop() {
  const [activeIndex, setActiveIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const reducedMotion = useReducedMotion();
  const activeStep = convergenceSteps[activeIndex];

  useEffect(() => {
    if (paused || reducedMotion) return undefined;
    const timer = window.setInterval(() => {
      setActiveIndex((current) => (current + 1) % convergenceSteps.length);
    }, 5_200);
    return () => window.clearInterval(timer);
  }, [paused, reducedMotion]);

  function activateTab(index: number) {
    setActiveIndex(index);
    setPaused(true);
    tabRefs.current[index]?.focus();
  }

  function handleTabKeyDown(
    event: React.KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) {
    const lastIndex = convergenceSteps.length - 1;
    const nextIndex =
      event.key === 'ArrowRight'
        ? (index + 1) % convergenceSteps.length
        : event.key === 'ArrowLeft'
          ? (index - 1 + convergenceSteps.length) % convergenceSteps.length
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? lastIndex
              : undefined;
    if (nextIndex === undefined) return;
    event.preventDefault();
    activateTab(nextIndex);
  }

  return (
    <div className="cloud-convergence-shell">
      <div
        className="cloud-convergence-rail"
        role="tablist"
        aria-label="Convergence stages"
      >
        {convergenceSteps.map((step, index) => (
          <button
            aria-controls="convergence-stage"
            aria-selected={activeIndex === index}
            className={activeIndex === index ? 'is-active' : ''}
            id={`convergence-tab-${index}`}
            key={step.code}
            onClick={() => activateTab(index)}
            onKeyDown={(event) => handleTabKeyDown(event, index)}
            ref={(element) => {
              tabRefs.current[index] = element;
            }}
            role="tab"
            tabIndex={activeIndex === index ? 0 : -1}
            type="button"
          >
            <span>{step.code}</span>
            <b>{step.name}</b>
            <i aria-hidden="true" />
          </button>
        ))}
      </div>

      <section
        aria-labelledby={`convergence-tab-${activeIndex}`}
        className="cloud-convergence-stage"
        id="convergence-stage"
        role="tabpanel"
      >
        <div className="cloud-convergence-stage-copy">
          <h3>{activeStep.system}</h3>
          <p>{activeStep.detail}</p>
        </div>
        <div className="cloud-convergence-console">
          <header>
            <span>Durable evidence</span>
            <button
              onClick={() => {
                if (reducedMotion) {
                  setActiveIndex((activeIndex + 1) % convergenceSteps.length);
                } else {
                  setPaused((current) => !current);
                }
              }}
              type="button"
            >
              {reducedMotion ? 'Next stage' : paused ? 'Resume' : 'Pause'}
            </button>
          </header>
          <div className="cloud-convergence-console-body" aria-live="polite">
            <span>Observed event</span>
            <strong>{activeStep.evidence}</strong>
            <small>
              <span>generation {String(activeIndex + 1).padStart(2, '0')}</span>
              <span>receipt durable</span>
              <span>replay safe</span>
            </small>
            <div className="cloud-convergence-meter" aria-hidden="true">
              {convergenceSteps.map((step, index) => (
                <i
                  className={index <= activeIndex ? 'is-complete' : ''}
                  key={step.code}
                />
              ))}
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
