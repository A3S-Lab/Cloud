import { AlertTriangle } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { ARCHITECTURE_GRAPH, type ArchitectureNode, type JourneyId } from '../architecture';
import {
  createArchitectureRuntime,
  type ArchitectureRuntime,
  type ArchitectureSimulationFrame,
} from '../scene/architecture-runtime';
import type { ArchitectureHoverEvent } from '../scene/interaction';

interface ArchitectureSceneProps {
  autoRotate: boolean;
  focusRevision: number;
  journey: JourneyId;
  resetRevision: number;
  selectedNodeId?: string;
  simulationFrame?: ArchitectureSimulationFrame;
  onSelectNode: (nodeId: string) => void;
}

export function ArchitectureScene({
  autoRotate,
  focusRevision,
  journey,
  resetRevision,
  selectedNodeId,
  simulationFrame,
  onSelectNode,
}: ArchitectureSceneProps) {
  const mountRef = useRef<HTMLDivElement>(null);
  const runtimeRef = useRef<ArchitectureRuntime | undefined>(undefined);
  const selectNodeRef = useRef(onSelectNode);
  selectNodeRef.current = onSelectNode;
  const [renderError, setRenderError] = useState<string>();
  const [hover, setHover] = useState<ArchitectureHoverEvent>();
  const previousFocusRevision = useRef(focusRevision);
  const previousResetRevision = useRef(resetRevision);
  const hoveredNode = hover ? ARCHITECTURE_GRAPH.nodes.find((node) => node.id === hover.nodeId) : undefined;

  // biome-ignore lint/correctness/useExhaustiveDependencies: The Three.js runtime is initialized once and synchronized by the focused effects below.
  useEffect(() => {
    const container = mountRef.current;
    if (!container) return;
    try {
      runtimeRef.current = createArchitectureRuntime(container, {
        graph: ARCHITECTURE_GRAPH,
        initialJourney: journey,
        initialSelectedNodeId: selectedNodeId,
        autoRotate,
        onHover: setHover,
        onSelect: (nodeId) => selectNodeRef.current(nodeId),
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
    runtimeRef.current?.setSelectedNode(selectedNodeId);
  }, [selectedNodeId]);

  useEffect(() => {
    runtimeRef.current?.setAutoRotate(autoRotate);
  }, [autoRotate]);

  useEffect(() => {
    runtimeRef.current?.setSimulationFrame(simulationFrame);
  }, [simulationFrame]);

  useEffect(() => {
    if (focusRevision === previousFocusRevision.current) return;
    previousFocusRevision.current = focusRevision;
    if (selectedNodeId) runtimeRef.current?.focusNode(selectedNodeId);
  }, [focusRevision, selectedNodeId]);

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

      {hoveredNode && hover ? <NodeTooltip node={hoveredNode} hover={hover} /> : null}

      {renderError ? <WebglFallback error={renderError} onSelectNode={onSelectNode} /> : null}
    </section>
  );
}

function NodeTooltip({ node, hover }: { node: ArchitectureNode; hover: ArchitectureHoverEvent }) {
  return (
    <output className={`node-tooltip is-${hover.placement}`} style={{ left: hover.x, top: hover.y }}>
      <span>{node.eyebrow}</span>
      <strong>{node.label}</strong>
      <small>{node.gate}</small>
    </output>
  );
}

function WebglFallback({ error, onSelectNode }: { error: string; onSelectNode: (nodeId: string) => void }) {
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
                  <button type='button' key={node.id} onClick={() => onSelectNode(node.id)}>
                    {node.label}
                  </button>
                ))}
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}
