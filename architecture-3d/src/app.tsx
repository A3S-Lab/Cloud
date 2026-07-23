import { Box, Github, Layers3, Network, Pause, Play, RotateCcw } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { ARCHITECTURE_GRAPH, ARCHITECTURE_STATUS_META, type JourneyId } from './architecture';
import { ArchifyScene } from './components/archify-scene';
import { ArchitectureScene } from './components/architecture-scene';
import { BusinessFlowPanel } from './components/business-flow-panel';
import { NodeInspector } from './components/node-inspector';
import { RelationshipInspector } from './components/relationship-inspector';
import type { ArchitectureRelationshipSelection, ArchitectureSelection } from './selection';
import {
  SIMULATION_SCENARIOS,
  type SimulationEntryId,
  type SimulationScenarioId,
  simulationFramesFor,
} from './simulations';

type ArchitectureViewMode = '3d' | '2d';

export function App() {
  const [journey, setJourney] = useState<JourneyId>('all');
  const [selection, setSelection] = useState<ArchitectureSelection>();
  const [viewMode, setViewMode] = useState<ArchitectureViewMode>('3d');
  const [autoRotate, setAutoRotate] = useState(false);
  const [focusRevision, setFocusRevision] = useState(0);
  const [resetRevision, setResetRevision] = useState(0);
  const [simulationEntryId, setSimulationEntryId] = useState<SimulationEntryId>('web');
  const [activeScenarioId, setActiveScenarioId] = useState<SimulationScenarioId>();
  const [simulationStepIndex, setSimulationStepIndex] = useState(0);
  const [simulationPlaying, setSimulationPlaying] = useState(false);

  const selectedNodeId = selection?.kind === 'node' ? selection.id : undefined;
  const selectedRelationship =
    selection?.kind === 'business-edge' || selection?.kind === 'structural-edge' ? selection : undefined;
  const selectedNode = useMemo(
    () => ARCHITECTURE_GRAPH.nodes.find((node) => node.id === selectedNodeId),
    [selectedNodeId]
  );
  const simulationFrames = useMemo(
    () => (activeScenarioId ? simulationFramesFor(simulationEntryId, activeScenarioId) : []),
    [activeScenarioId, simulationEntryId]
  );
  const simulationFrame = simulationFrames[simulationStepIndex];

  const selectAndFocusNode = (nodeId: string) => {
    setSimulationPlaying(false);
    setSelection({ kind: 'node', id: nodeId });
    setFocusRevision((revision) => revision + 1);
  };

  const selectRelationship = (nextSelection: ArchitectureRelationshipSelection) => {
    setSimulationPlaying(false);
    setSelection(nextSelection);
  };

  useEffect(() => {
    if (!simulationPlaying || !simulationFrame) return;
    const timer = window.setTimeout(() => {
      if (simulationStepIndex >= simulationFrames.length - 1) {
        setSimulationPlaying(false);
        return;
      }
      setSimulationStepIndex((index) => index + 1);
    }, simulationFrame.durationMs);
    return () => window.clearTimeout(timer);
  }, [simulationFrame, simulationFrames.length, simulationPlaying, simulationStepIndex]);

  const startScenario = (scenarioId: SimulationScenarioId) => {
    const scenario = SIMULATION_SCENARIOS.find((candidate) => candidate.id === scenarioId);
    setActiveScenarioId(scenarioId);
    setSimulationStepIndex(0);
    setSimulationPlaying(true);
    setJourney(scenario?.journey ?? 'all');
    setSelection(undefined);
  };

  const stopSimulation = () => {
    setActiveScenarioId(undefined);
    setSimulationPlaying(false);
    setSimulationStepIndex(0);
    setJourney('all');
  };

  return (
    <div className='app-shell'>
      <a className='skip-link' href='#architecture-map'>
        Skip to architecture map
      </a>

      <header className='top-bar'>
        <a className='brand-lockup' href='./' aria-label='A3S Cloud architecture home'>
          <span className='brand-mark' aria-hidden='true'>
            <i />
            <i />
            <i />
          </span>
          <span>
            <strong>A3S Cloud</strong>
            <small>Interactive architecture</small>
          </span>
        </a>

        <output className='roadmap-readout' aria-label='Current product roadmap status'>
          <span>
            <i className='is-verified' />
            R0–E0 verified
          </span>
          <span>
            <i className='is-progress' />
            G0 in progress
          </span>
          <span>
            <i className='is-planned' />
            I0 planned
          </span>
        </output>

        <a
          className='github-link'
          href='https://github.com/A3S-Lab/Cloud'
          target='_blank'
          rel='noreferrer'
          aria-label='View A3S Cloud repository'
        >
          <Github size={16} aria-hidden='true' />
          <span>View repository</span>
        </a>
      </header>

      <main id='architecture-map' className={`architecture-map is-${viewMode}`}>
        {viewMode === '3d' ? (
          <>
            <ArchitectureScene
              autoRotate={autoRotate}
              focusRevision={focusRevision}
              journey={journey}
              resetRevision={resetRevision}
              selection={selection}
              simulationFrame={simulationFrame}
              onSelect={(nextSelection) => {
                setSimulationPlaying(false);
                setSelection(nextSelection);
              }}
            />

            <BusinessFlowPanel
              activeScenarioId={activeScenarioId}
              entryId={simulationEntryId}
              isPlaying={simulationPlaying}
              journey={journey}
              stepIndex={simulationStepIndex}
              onChangeEntry={(entryId) => {
                setSimulationEntryId(entryId);
                if (activeScenarioId) {
                  setSimulationStepIndex(0);
                  setSimulationPlaying(true);
                }
              }}
              onChangeJourney={(nextJourney) => {
                setJourney(nextJourney);
                setActiveScenarioId(undefined);
                setSimulationPlaying(false);
                setSimulationStepIndex(0);
              }}
              onNext={() => {
                setSimulationPlaying(false);
                setSimulationStepIndex((index) => Math.min(index + 1, simulationFrames.length - 1));
              }}
              onPrevious={() => {
                setSimulationPlaying(false);
                setSimulationStepIndex((index) => Math.max(index - 1, 0));
              }}
              onSelectStep={(index) => {
                setSimulationPlaying(false);
                setSimulationStepIndex(index);
              }}
              onStartScenario={startScenario}
              onStop={stopSimulation}
              onTogglePlayback={() => {
                if (!simulationPlaying && simulationStepIndex >= simulationFrames.length - 1) {
                  setSimulationStepIndex(0);
                }
                setSimulationPlaying((playing) => !playing);
              }}
            />
          </>
        ) : (
          <ArchifyScene
            selection={selection}
            onClearSelection={() => setSelection(undefined)}
            onSelect={(nextSelection) => {
              setSimulationPlaying(false);
              setSelection(nextSelection);
            }}
          />
        )}

        <section className='component-picker' aria-labelledby='component-picker-label'>
          <label id='component-picker-label' htmlFor='component-picker'>
            <Layers3 size={13} aria-hidden='true' />
            Find a component
          </label>
          <div className='select-wrap'>
            <select
              id='component-picker'
              aria-label='Find a component'
              value={selectedNodeId ?? ''}
              onChange={(event) => {
                if (event.target.value) selectAndFocusNode(event.target.value);
              }}
            >
              <option value='' disabled>
                Select a component
              </option>
              {ARCHITECTURE_GRAPH.domains.map((domain) => (
                <optgroup key={domain.id} label={domain.label}>
                  {ARCHITECTURE_GRAPH.nodes
                    .filter((node) => node.domain === domain.id)
                    .map((node) => (
                      <option key={node.id} value={node.id}>
                        {node.label} · {node.gate}
                      </option>
                    ))}
                </optgroup>
              ))}
            </select>
          </div>
          <fieldset className='status-legend'>
            <legend className='sr-only'>Roadmap status legend</legend>
            {Object.entries(ARCHITECTURE_STATUS_META).map(([id, status]) => (
              <span key={id} title={status.description}>
                <i style={{ background: status.color }} aria-hidden='true' />
                {status.label}
              </span>
            ))}
          </fieldset>
          <fieldset className='topology-legend'>
            <legend className='sr-only'>Architecture relationship legend</legend>
            <span>
              <i className='is-business-flow' aria-hidden='true' />
              Business flow
            </span>
            <span>
              <i className='is-hosting-link' aria-hidden='true' />
              Structure / hosting
            </span>
            <span>
              <i className='is-carrier-frame' aria-hidden='true' />
              Carrier chassis
            </span>
          </fieldset>
        </section>

        <fieldset className='view-controls'>
          <legend className='sr-only'>Architecture view controls</legend>
          <button
            type='button'
            className={viewMode === '3d' ? 'is-active' : undefined}
            onClick={() => setViewMode('3d')}
            aria-pressed={viewMode === '3d'}
            aria-label='Show interactive 3D architecture'
          >
            <Box size={15} aria-hidden='true' />
            <span>3D</span>
          </button>
          <button
            type='button'
            className={viewMode === '2d' ? 'is-active' : undefined}
            onClick={() => {
              setViewMode('2d');
              setSimulationPlaying(false);
            }}
            aria-pressed={viewMode === '2d'}
            aria-label='Show interactive 2D Archify architecture'
          >
            <Network size={15} aria-hidden='true' />
            <span>2D</span>
          </button>
          {viewMode === '3d' ? (
            <>
              <button
                type='button'
                className={autoRotate ? 'is-active' : undefined}
                onClick={() => setAutoRotate((enabled) => !enabled)}
                aria-pressed={autoRotate}
                aria-label={autoRotate ? 'Pause orbit' : 'Auto orbit'}
              >
                {autoRotate ? <Pause size={15} aria-hidden='true' /> : <Play size={15} aria-hidden='true' />}
                <span>{autoRotate ? 'Pause orbit' : 'Auto orbit'}</span>
              </button>
              <button
                type='button'
                aria-label='Reset view'
                onClick={() => setResetRevision((revision) => revision + 1)}
              >
                <RotateCcw size={15} aria-hidden='true' />
                <span>Reset view</span>
              </button>
            </>
          ) : null}
        </fieldset>

        <div className='interaction-hint' aria-hidden='true'>
          {viewMode === '3d' ? <Box size={13} /> : <Network size={13} />}
          {viewMode === '3d'
            ? 'Drag to orbit · Scroll to zoom · Click a component or relationship'
            : 'Pan and zoom · Click nodes or relationships · Use PATH, LENS, and MAP'}
        </div>

        <NodeInspector
          node={selectedNode}
          onClose={() => setSelection(undefined)}
          onFocus={() => setFocusRevision((revision) => revision + 1)}
          onSelectRelationship={selectRelationship}
        />

        <RelationshipInspector
          selection={selectedRelationship}
          onClose={() => setSelection(undefined)}
          onSelectNode={selectAndFocusNode}
        />
      </main>
    </div>
  );
}
