import {
  ChevronLeft,
  ChevronRight,
  Filter,
  Monitor,
  Pause,
  Play,
  RotateCcw,
  Square,
  Terminal,
  Workflow,
} from 'lucide-react';
import type { CSSProperties } from 'react';
import { JOURNEYS, type JourneyId } from '../architecture';
import {
  SIMULATION_ENTRIES,
  SIMULATION_SCENARIOS,
  type SimulationEntryId,
  type SimulationScenarioId,
  simulationFramesFor,
} from '../simulations';

interface BusinessFlowPanelProps {
  activeScenarioId?: SimulationScenarioId;
  entryId: SimulationEntryId;
  isPlaying: boolean;
  journey: JourneyId;
  stepIndex: number;
  onChangeEntry: (entryId: SimulationEntryId) => void;
  onChangeJourney: (journey: JourneyId) => void;
  onNext: () => void;
  onPrevious: () => void;
  onSelectStep: (stepIndex: number) => void;
  onStartScenario: (scenarioId: SimulationScenarioId) => void;
  onStop: () => void;
  onTogglePlayback: () => void;
}

export function BusinessFlowPanel({
  activeScenarioId,
  entryId,
  isPlaying,
  journey,
  stepIndex,
  onChangeEntry,
  onChangeJourney,
  onNext,
  onPrevious,
  onSelectStep,
  onStartScenario,
  onStop,
  onTogglePlayback,
}: BusinessFlowPanelProps) {
  const activeScenario = SIMULATION_SCENARIOS.find((scenario) => scenario.id === activeScenarioId);
  const frames = activeScenarioId ? simulationFramesFor(entryId, activeScenarioId) : [];
  const activeFrame = frames[stepIndex];

  return (
    <section className='simulation-panel' aria-labelledby='simulation-title'>
      <div className='simulation-heading'>
        <div className='panel-label'>
          <Workflow size={13} aria-hidden='true' />
          <span id='simulation-title'>Business flow simulator</span>
        </div>
        {activeFrame ? (
          <span className={`simulation-live ${isPlaying ? 'is-playing' : ''}`}>
            <i aria-hidden='true' />
            {isPlaying ? 'Running' : 'Paused'} · {stepIndex + 1}/{frames.length}
          </span>
        ) : (
          <span className='simulation-live'>Choose a scenario</span>
        )}
      </div>

      <div className='simulation-entry-row'>
        <span>Operate from</span>
        <fieldset className='entry-switch'>
          <legend className='sr-only'>Simulation management surface</legend>
          {SIMULATION_ENTRIES.map((entry) => (
            <button
              type='button'
              key={entry.id}
              className={entry.id === entryId ? 'is-active' : undefined}
              onClick={() => onChangeEntry(entry.id)}
              aria-pressed={entry.id === entryId}
            >
              {entry.id === 'web' ? (
                <Monitor size={12} aria-hidden='true' />
              ) : (
                <Terminal size={12} aria-hidden='true' />
              )}
              {entry.shortLabel}
            </button>
          ))}
        </fieldset>
      </div>

      <fieldset className='scenario-buttons'>
        <legend className='sr-only'>Business simulation scenarios</legend>
        {SIMULATION_SCENARIOS.map((scenario) => (
          <button
            type='button'
            key={scenario.id}
            className={scenario.id === activeScenarioId ? 'is-active' : undefined}
            style={{ '--scenario-color': scenario.color } as CSSProperties}
            onClick={() => onStartScenario(scenario.id)}
            aria-pressed={scenario.id === activeScenarioId}
            title={scenario.label}
          >
            <i aria-hidden='true' />
            {scenario.shortLabel}
          </button>
        ))}
      </fieldset>

      {activeScenario && activeFrame ? (
        <div
          className='simulation-stage'
          style={{ '--scenario-color': activeScenario.color } as CSSProperties}
        >
          <div className='simulation-copy'>
            <span>{activeFrame.actor}</span>
            <h2>{activeFrame.title}</h2>
            <p>{activeFrame.description}</p>
          </div>

          <div className='simulation-controls'>
            <button
              type='button'
              onClick={onPrevious}
              disabled={stepIndex === 0}
              aria-label='Previous simulation step'
            >
              <ChevronLeft size={14} aria-hidden='true' />
            </button>
            <button
              type='button'
              className='simulation-play'
              onClick={onTogglePlayback}
              aria-label={isPlaying ? 'Pause simulation' : 'Play simulation'}
            >
              {isPlaying ? <Pause size={14} aria-hidden='true' /> : <Play size={14} aria-hidden='true' />}
            </button>
            <button
              type='button'
              onClick={onNext}
              disabled={stepIndex >= frames.length - 1}
              aria-label='Next simulation step'
            >
              <ChevronRight size={14} aria-hidden='true' />
            </button>
            <button
              type='button'
              onClick={() => onStartScenario(activeScenario.id)}
              aria-label='Replay simulation'
            >
              <RotateCcw size={13} aria-hidden='true' />
            </button>
            <button type='button' onClick={onStop} aria-label='Stop simulation'>
              <Square size={12} aria-hidden='true' />
            </button>
          </div>

          <fieldset className='simulation-timeline'>
            <legend className='sr-only'>Simulation steps</legend>
            {frames.map((frame, index) => (
              <button
                type='button'
                key={frame.id}
                className={index === stepIndex ? 'is-active' : index < stepIndex ? 'is-complete' : undefined}
                onClick={() => onSelectStep(index)}
                aria-label={`Go to step ${index + 1}: ${frame.title}`}
                aria-current={index === stepIndex ? 'step' : undefined}
              >
                <i aria-hidden='true' />
              </button>
            ))}
          </fieldset>
        </div>
      ) : (
        <p className='simulation-intro'>
          Run a scenario to watch commands, durable state, provider execution, CPU/GPU hardware, traffic, and
          observations move across the sandbox.
        </p>
      )}

      <div className='path-filter'>
        <span>
          <Filter size={11} aria-hidden='true' />
          Path filter
        </span>
        <fieldset>
          <legend className='sr-only'>Architecture path filter</legend>
          {JOURNEYS.map((candidate) => (
            <button
              type='button'
              key={candidate.id}
              className={candidate.id === journey ? 'is-active' : undefined}
              onClick={() => onChangeJourney(candidate.id)}
              aria-pressed={candidate.id === journey}
            >
              {candidate.shortLabel}
            </button>
          ))}
        </fieldset>
      </div>
    </section>
  );
}
