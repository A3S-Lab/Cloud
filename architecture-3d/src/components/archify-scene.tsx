import { ExternalLink } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { ARCHIFY_ARTIFACT_PATH, archifyRelationSelection, primaryArchifyRelationId } from '../archify-bridge';
import { ARCHITECTURE_GRAPH } from '../architecture';
import type { ArchitectureSelection } from '../selection';

interface ArchifySceneProps {
  selection?: ArchitectureSelection;
  onClearSelection: () => void;
  onSelect: (selection: ArchitectureSelection) => void;
}

export function ArchifyScene({ selection, onClearSelection, onSelect }: ArchifySceneProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const childWindowRef = useRef<Window | undefined>(undefined);
  const detachBridgeRef = useRef<(() => void) | undefined>(undefined);
  const selectionRef = useRef(selection);
  const selectRef = useRef(onSelect);
  const clearRef = useRef(onClearSelection);
  const [loadError, setLoadError] = useState<string>();
  selectionRef.current = selection;
  selectRef.current = onSelect;
  clearRef.current = onClearSelection;

  useEffect(
    () => () => {
      detachBridgeRef.current?.();
    },
    []
  );

  useEffect(() => {
    const childWindow = childWindowRef.current;
    if (childWindow) syncSelectionToArchify(childWindow, selection);
  }, [selection]);

  const handleLoad = () => {
    detachBridgeRef.current?.();
    const childWindow = iframeRef.current?.contentWindow;
    if (!childWindow) return;
    if (!childWindow.document.querySelector('.diagram-container svg[data-preset]')) {
      setLoadError(
        'The generated Archify artifact is unavailable. Run the Archify source and render workflow before starting the site.'
      );
      childWindowRef.current = undefined;
      return;
    }
    setLoadError(undefined);
    childWindowRef.current = childWindow;
    syncSelectionToArchify(childWindow, selectionRef.current);

    const handleHashChange = () => {
      const nextSelection = selectionFromArchifyHash(childWindow.location.hash);
      if (nextSelection) {
        selectRef.current(nextSelection);
      } else {
        clearRef.current();
      }
    };
    childWindow.addEventListener('hashchange', handleHashChange);
    const hashObserver = childWindow.setInterval(handleHashChange, 100);
    detachBridgeRef.current = () => {
      childWindow.removeEventListener('hashchange', handleHashChange);
      childWindow.clearInterval(hashObserver);
      if (childWindowRef.current === childWindow) childWindowRef.current = undefined;
    };
  };

  const artifactUrl = `${import.meta.env.BASE_URL}${ARCHIFY_ARTIFACT_PATH}?theme=dark`;

  return (
    <section className='archify-viewport' aria-label='A3S Cloud interactive 2D architecture'>
      <iframe
        ref={iframeRef}
        src={artifactUrl}
        title='A3S Cloud 2D architecture powered by Archify'
        allow='clipboard-write'
        onLoad={handleLoad}
      />
      {loadError ? (
        <div className='archify-load-error' role='alert'>
          <strong>2D architecture could not load</strong>
          <p>{loadError}</p>
        </div>
      ) : null}
      <a
        className='archify-attribution'
        href='https://github.com/tt-a1i/archify'
        target='_blank'
        rel='noreferrer'
      >
        Interactive 2D view powered by Archify
        <ExternalLink size={12} aria-hidden='true' />
      </a>
    </section>
  );
}

function selectionFromArchifyHash(hash: string): ArchitectureSelection | undefined {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const focusedNodeId = params.get('focus');
  if (focusedNodeId && ARCHITECTURE_GRAPH.nodes.some((node) => node.id === focusedNodeId)) {
    return { kind: 'node', id: focusedNodeId };
  }
  const relationId = params.get('relation');
  return relationId ? archifyRelationSelection(relationId) : undefined;
}

function syncSelectionToArchify(childWindow: Window, selection?: ArchitectureSelection): void {
  const params = new URLSearchParams(childWindow.location.hash.replace(/^#/, ''));
  params.delete('focus');
  params.delete('relation');
  params.delete('reach');

  if (selection?.kind === 'node') {
    params.set('focus', selection.id);
  } else if (selection) {
    const relationId = primaryArchifyRelationId(selection);
    if (relationId) params.set('relation', relationId);
  }

  const nextHash = params.toString();
  const currentHash = childWindow.location.hash.replace(/^#/, '');
  if (nextHash !== currentHash) childWindow.location.hash = nextHash;
}
