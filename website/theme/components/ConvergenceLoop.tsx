import { useEffect, useState } from 'react';
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
  const reducedMotion = useReducedMotion();
  const activeStep = convergenceSteps[activeIndex];

  useEffect(() => {
    if (paused || reducedMotion) return undefined;
    const timer = window.setInterval(() => {
      setActiveIndex((current) => (current + 1) % convergenceSteps.length);
    }, 2_150);
    return () => window.clearInterval(timer);
  }, [paused, reducedMotion]);

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
            onClick={() => {
              setActiveIndex(index);
              setPaused(true);
            }}
            role="tab"
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
          <span>ACTIVE SYSTEM</span>
          <h3>{activeStep.system}</h3>
          <p>{activeStep.detail}</p>
        </div>
        <div className="cloud-convergence-console" aria-live="polite">
          <header>
            <span aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
            convergence.trace
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
          <div className="cloud-convergence-console-body">
            <span>$ cloud observe operation</span>
            <strong>{activeStep.evidence}</strong>
            <small>
              generation {String(activeIndex + 1).padStart(2, '0')} · receipt
              durable · replay safe
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
