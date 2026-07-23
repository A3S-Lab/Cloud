import { AlertTriangle } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { ARCHITECTURE_GRAPH, type JourneyId } from '../architecture';
import type { ArchitectureSelection } from '../selection';
import {
  createArchitectureRuntime,
  type ArchitectureRuntime,
  type ArchitectureSimulationFrame,
} from '../scene/architecture-runtime';
import type { ArchitectureHoverEvent } from '../scene/interaction';
import { ARCHITECTURE_HOSTING_RELATIONSHIPS } from '../topology';

interface ArchitectureSceneProps {
  autoRotate: boolean;
  focusRevision: number;
  journey: JourneyId;
  resetRevision: number;
  selection?: ArchitectureSelection;
  simulationFrame?: ArchitectureSimulationFrame;
  onSelect: (selection: ArchitectureSelection) => void;
}

export function ArchitectureScene({
  autoRotate,
  focusRevision,
  journey,
  resetRevision,
  selection,
  simulationFrame,
  onSelect,
}: ArchitectureSceneProps) {
  const mountRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<ArchitectureRuntime | undefined>(undefined);
  const selectRef = useRef(onSelect);
  selectRef.current = onSelect;
  const [renderError, setRenderError] = useState<string>();
  const [hover, setHover] = useState<ArchitectureHoverEvent>();
  const previousFocusRevision = useRef(focusRevision);
  const previousResetRevision = useRef(resetRevision);

  // biome-ignore lint/correctness/useExhaustiveDependencies: The Three.js runtime is initialized once and synchronized by the focused effects below.
  useEffect(() => {
    const container = mountRef.current;
    if (!container) return;
    try {
      runtimeRef.current = createArchitectureRuntime(container, {
        graph: ARCHITECTURE_GRAPH,
        initialJourney: journey,
        initialSelection: selection,
        autoRotate,
        onHover: setHover,
        onSelect: (nextSelection) => selectRef.current(nextSelection),
      });
      setRenderError(undefined);
    } catch (error) {
      setRenderError(
        error instanceof Error ? error.message : 'The WebGL architecture scene could not start.'
      );
    }

    return () => {
      runtimeRef.current?.dispose();
      runtimeRef.current = undefined;
    };
  }, []);

  useEffect(() => {
    runtimeRef.current?.setJourney(journey);
  }, [journey]);

  useEffect(() => {
    runtimeRef.current?.setSelection(selection);
  }, [selection]);

  useEffect(() => {
    runtimeRef.current?.setAutoRotate(autoRotate);
  }, [autoRotate]);

  useEffect(() => {
    runtimeRef.current?.setSimulationFrame(simulationFrame);
  }, [simulationFrame]);

  useEffect(() => {
    if (focusRevision === previousFocusRevision.current) return;
    previousFocusRevision.current = focusRevision;
    if (selection?.kind === 'node') runtimeRef.current?.focusNode(selection.id);
  }, [focusRevision, selection]);

  useEffect(() => {
    if (resetRevision === previousResetRevision.current) return;
    previousResetRevision.current = resetRevision;
    runtimeRef.current?.resetCamera();
  }, [resetRevision]);

  return (
    <section className='architecture-viewport' aria-label='A3S Cloud interactive architecture'>
      <div className='three-mount' ref={mountRef} data-testid='architecture-canvas' />
      <div className='viewport-vignette' aria-hidden='true' />
      <div className='viewport-grid' aria-hidden='true' />
      <div className='viewport-corner viewport-corner-tl' aria-hidden='true' />
      <div className='viewport-corner viewport-corner-tr' aria-hidden='true' />
      <div className='viewport-corner viewport-corner-bl' aria-hidden='true' />
      <div className='viewport-corner viewport-corner-br' aria-hidden='true' />

      {hover ? <SelectionTooltip hover={hover} /> : null}

      {renderError ? <WebglFallback error={renderError} onSelect={onSelect} /> : null}
    </section>
  );
}

function SelectionTooltip({ hover }: { hover: ArchitectureHoverEvent }) {
  const { selection } = hover;
  if (selection.kind === 'node') {
    const node = ARCHITECTURE_GRAPH.nodes.find((candidate) => candidate.id === selection.id);
    if (!node) return null;
    return (
      <output className={`node-tooltip is-${hover.placement}`} style={{ left: hover.x, top: hover.y }}>
        <span>{node.eyebrow}</span>
        <strong>{node.label}</strong>
        <small>{node.gate}</small>
      </output>
    );
  }

  const businessEdge =
    selection.kind === 'business-edge'
      ? ARCHITECTURE_GRAPH.edges.find((edge) => edge.id === selection.id)
      : undefined;
  const structuralEdge =
    selection.kind === 'structural-edge'
      ? ARCHITECTURE_HOSTING_RELATIONSHIPS.find((relationship) => relationship.id === selection.id)
      : undefined;
  const sourceLabels = businessEdge
    ? [ARCHITECTURE_GRAPH.nodes.find((node) => node.id === businessEdge.from)?.label]
    : structuralEdge?.hostNodeIds.map(
        (nodeId) => ARCHITECTURE_GRAPH.nodes.find((node) => node.id === nodeId)?.label
      );
  const targetLabels = businessEdge
    ? [ARCHITECTURE_GRAPH.nodes.find((node) => node.id === businessEdge.to)?.label]
    : structuralEdge?.guestNodeIds.map(
        (nodeId) => ARCHITECTURE_GRAPH.nodes.find((node) => node.id === nodeId)?.label
      );
  const label = businessEdge?.label ?? structuralEdge?.label;
  if (!label) return null;

  return (
    <output className={`node-tooltip is-${hover.placement}`} style={{ left: hover.x, top: hover.y }}>
      <span>{businessEdge ? 'Business flow' : 'Structure / hosting'}</span>
      <strong>{label}</strong>
      <small>
        {sourceLabels?.filter(Boolean).join(', ')} → {targetLabels?.filter(Boolean).join(', ')}
      </small>
    </output>
  );
}

function WebglFallback({
  error,
  onSelect,
}: {
  error: string;
  onSelect: (selection: ArchitectureSelection) => void;
}) {
  return (
    <div className='webgl-fallback' role='alert'>
      <div className='webgl-fallback-heading'>
        <AlertTriangle size={22} aria-hidden='true' />
        <div>
          <strong>3D rendering is unavailable</strong>
          <p>{error}</p>
        </div>
      </div>
      <p className='fallback-instruction'>
        The complete architecture remains available as an accessible component index.
      </p>
      <div className='fallback-layers'>
        {ARCHITECTURE_GRAPH.domains.map((domain) => (
          <section key={domain.id}>
            <h2>{domain.label}</h2>
            <div>
              {ARCHITECTURE_GRAPH.nodes
                .filter((node) => node.domain === domain.id)
                .map((node) => (
                  <button type='button' key={node.id} onClick={() => onSelect({ kind: 'node', id: node.id })}>
                    {node.label}
                  </button>
                ))}
            </div>
          </section>
        ))}
        <section>
          <h2>Business flows</h2>
          <div>
            {ARCHITECTURE_GRAPH.edges.map((edge) => (
              <button
                type='button'
                key={edge.id}
                onClick={() => onSelect({ kind: 'business-edge', id: edge.id })}
              >
                {edge.label}
              </button>
            ))}
          </div>
        </section>
        <section>
          <h2>Structure & hosting</h2>
          <div>
            {ARCHITECTURE_HOSTING_RELATIONSHIPS.map((relationship) => (
              <button
                type='button'
                key={relationship.id}
                onClick={() => onSelect({ kind: 'structural-edge', id: relationship.id })}
              >
                {relationship.label}
              </button>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
