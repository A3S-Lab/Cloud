import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { App } from './app';
import type { JourneyId } from './architecture';
import type { ArchitectureSimulationFrame } from './scene/architecture-runtime';
import type { ArchitectureSelection } from './selection';

vi.mock('./components/architecture-scene', () => ({
  ArchitectureScene: ({
    focusRevision,
    journey,
    onSelect,
    resetRevision,
    simulationFrame,
  }: {
    focusRevision: number;
    journey: JourneyId;
    onSelect: (selection: ArchitectureSelection) => void;
    resetRevision: number;
    simulationFrame?: ArchitectureSimulationFrame;
  }) => (
    <div
      data-testid='architecture-scene'
      data-focus-revision={focusRevision}
      data-journey={journey}
      data-reset-revision={resetRevision}
      data-simulation-nodes={simulationFrame?.nodeIds.join(',')}
      data-simulation-edges={simulationFrame?.edgeIds.join(',')}
    >
      <button type='button' onClick={() => onSelect({ kind: 'node', id: 'gateway' })}>
        Select Gateway in scene
      </button>
      <button type='button' onClick={() => onSelect({ kind: 'business-edge', id: 'web-gateway' })}>
        Select Web to Gateway flow
      </button>
      <button type='button' onClick={() => onSelect({ kind: 'structural-edge', id: 'box-hosts-code' })}>
        Select Box hosting relationship
      </button>
    </div>
  ),
}));

vi.mock('./components/archify-scene', () => ({
  ArchifyScene: ({ onSelect }: { onSelect: (selection: ArchitectureSelection) => void }) => (
    <div data-testid='archify-scene'>
      <button type='button' onClick={() => onSelect({ kind: 'node', id: 'power' })}>
        Select Power in 2D
      </button>
    </div>
  ),
}));

describe('A3S Cloud architecture application', () => {
  it('switches journeys and exposes the selected flow to the scene', () => {
    render(<App />);

    fireEvent.click(screen.getByRole('button', { name: 'Traffic' }));

    expect(screen.getByTestId('architecture-scene')).toHaveAttribute('data-journey', 'traffic');
    expect(screen.getByRole('button', { name: 'Traffic' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('runs business simulations from both A3S Web and A3S Code TUI', () => {
    render(<App />);

    fireEvent.click(screen.getByRole('button', { name: 'CPU deploy' }));
    expect(screen.getByTestId('architecture-scene')).toHaveAttribute('data-journey', 'deploy');
    expect(screen.getByTestId('architecture-scene')).toHaveAttribute(
      'data-simulation-nodes',
      'clients,web,gateway,api'
    );
    expect(screen.getByRole('button', { name: 'Pause simulation' })).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Code TUI' }));
    expect(screen.getByTestId('architecture-scene')).toHaveAttribute(
      'data-simulation-nodes',
      'a3s-box,code-tui,gateway,api'
    );

    fireEvent.click(screen.getByRole('button', { name: 'Next simulation step' }));
    expect(screen.getByTestId('architecture-scene')).toHaveAttribute(
      'data-simulation-nodes',
      'api,identity,projects'
    );
    expect(screen.getByRole('button', { name: 'Play simulation' })).toBeVisible();
  });

  it('selects components from both the index and the scene without resizing the map', () => {
    render(<App />);
    const componentPicker = screen.getByRole('combobox', { name: 'Find a component' });

    fireEvent.change(componentPicker, { target: { value: 'runtime' } });
    expect(screen.getByRole('heading', { name: 'A3S Runtime' })).toBeVisible();
    expect(screen.getByTestId('architecture-scene')).toHaveAttribute('data-focus-revision', '1');

    fireEvent.click(screen.getByRole('button', { name: 'Select Gateway in scene' }));
    expect(screen.getByRole('heading', { name: 'A3S Gateway' })).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Close component details' }));
    expect(screen.queryByRole('heading', { name: 'A3S Gateway' })).not.toBeInTheDocument();
  });

  it('opens detailed HUDs for business and structural relationships', () => {
    render(<App />);

    fireEvent.click(screen.getByRole('button', { name: 'Select Web to Gateway flow' }));
    expect(screen.getByRole('heading', { name: 'same-origin /api requests' })).toBeVisible();
    expect(screen.getByText('What crosses this boundary')).toBeVisible();
    expect(screen.getByText('HTTPS /api request')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Close relationship details' }));
    fireEvent.click(screen.getByRole('button', { name: 'Select Box hosting relationship' }));
    expect(screen.getByRole('heading', { name: 'A3S Code as one local workload' })).toBeVisible();
    expect(screen.getByText('Directional semantics')).toBeVisible();
    expect(screen.getAllByText('runs inside')).toHaveLength(2);
  });

  it('switches between the 3D and Archify 2D tabs', () => {
    render(<App />);

    expect(screen.getByRole('button', { name: 'Show interactive 3D architecture' })).toHaveTextContent('3D');
    expect(
      screen.getByRole('button', { name: 'Show interactive 2D Archify architecture' })
    ).toHaveTextContent('2D');
    fireEvent.click(screen.getByRole('button', { name: 'Show interactive 2D Archify architecture' }));
    expect(screen.getByTestId('archify-scene')).toBeVisible();

    fireEvent.click(screen.getByRole('button', { name: 'Select Power in 2D' }));
    expect(screen.getByRole('heading', { name: 'A3S Power' })).toBeVisible();
  });

  it('toggles camera motion and requests a reset', () => {
    render(<App />);

    fireEvent.click(screen.getByRole('button', { name: 'Auto orbit' }));
    expect(screen.getByRole('button', { name: 'Pause orbit' })).toHaveAttribute('aria-pressed', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'Reset view' }));
    expect(screen.getByTestId('architecture-scene')).toHaveAttribute('data-reset-revision', '1');
  });
});
