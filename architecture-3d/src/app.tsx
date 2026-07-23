import { Box, Github, Layers3, Pause, Play, RotateCcw, Route } from 'lucide-react';
import { useMemo, useState } from 'react';
import { ARCHITECTURE_GRAPH, ARCHITECTURE_STATUS_META, JOURNEYS, type JourneyId } from './architecture';
import { ArchitectureScene } from './components/architecture-scene';
import { NodeInspector } from './components/node-inspector';

const INITIAL_NODE_ID = 'workloads';

export function App() {
  const [journey, setJourney] = useState<JourneyId>('all');
  const [selectedNodeId, setSelectedNodeId] = useState<string | undefined>(INITIAL_NODE_ID);
  const [autoRotate, setAutoRotate] = useState(false);
  const [focusRevision, setFocusRevision] = useState(0);
  const [resetRevision, setResetRevision] = useState(0);

  const selectedNode = useMemo(
    () => ARCHITECTURE_GRAPH.nodes.find((node) => node.id === selectedNodeId),
    [selectedNodeId]
  );
  const activeJourney = JOURNEYS.find((candidate) => candidate.id === journey) ?? JOURNEYS[0];

  const selectAndFocusNode = (nodeId: string) => {
    setSelectedNodeId(nodeId);
    setFocusRevision((revision) => revision + 1);
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

      <main id='architecture-map' className='architecture-map'>
        <ArchitectureScene
          autoRotate={autoRotate}
          focusRevision={focusRevision}
          journey={journey}
          resetRevision={resetRevision}
          selectedNodeId={selectedNodeId}
          onSelectNode={setSelectedNodeId}
        />

        <section className='journey-panel' aria-labelledby='journey-title'>
          <div className='panel-label'>
            <Route size={13} aria-hidden='true' />
            <span id='journey-title'>Trace a system journey</span>
          </div>
          <fieldset className='journey-tabs'>
            <legend className='sr-only'>Architecture journey</legend>
            {JOURNEYS.map((candidate) => (
              <button
                type='button'
                key={candidate.id}
                className={candidate.id === journey ? 'is-active' : undefined}
                onClick={() => setJourney(candidate.id)}
                aria-pressed={candidate.id === journey}
              >
                {candidate.shortLabel}
              </button>
            ))}
          </fieldset>
          <p>{activeJourney.description}</p>
        </section>

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
              {ARCHITECTURE_GRAPH.layers.map((layer) => (
                <optgroup key={layer.id} label={layer.label}>
                  {ARCHITECTURE_GRAPH.nodes
                    .filter((node) => node.layer === layer.id)
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
        </section>

        <fieldset className='view-controls'>
          <legend className='sr-only'>3D view controls</legend>
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
        </fieldset>

        <div className='interaction-hint' aria-hidden='true'>
          <Box size={13} />
          Drag to orbit · Scroll to zoom · Click a component
        </div>

        <NodeInspector
          node={selectedNode}
          onClose={() => setSelectedNodeId(undefined)}
          onFocus={() => setFocusRevision((revision) => revision + 1)}
          onSelectNode={selectAndFocusNode}
        />
      </main>
    </div>
  );
}
