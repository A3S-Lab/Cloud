import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { App } from './app';
import type { JourneyId } from './architecture';

vi.mock('./components/architecture-scene', () => ({
  ArchitectureScene: ({
    focusRevision,
    journey,
    onSelectNode,
    resetRevision,
  }: {
    focusRevision: number;
    journey: JourneyId;
    onSelectNode: (nodeId: string) => void;
    resetRevision: number;
  }) => (
    <div
      data-testid='architecture-scene'
      data-focus-revision={focusRevision}
      data-journey={journey}
      data-reset-revision={resetRevision}
    >
      <button type='button' onClick={() => onSelectNode('gateway')}>
        Select Gateway in scene
      </button>
    </div>
  ),
}));

describe('A3S Cloud architecture application', () => {
  it('switches journeys and exposes the selected flow to the scene', () => {
    render(<App />);

    fireEvent.click(screen.getByRole('button', { name: 'Traffic' }));

    expect(screen.getByTestId('architecture-scene')).toHaveAttribute('data-journey', 'traffic');
    expect(
      screen.getByText(
        'Clients reach only Gateway-selected healthy targets; Cloud stays off the request path.'
      )
    ).toBeVisible();
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

  it('toggles camera motion and requests a reset', () => {
    render(<App />);

    fireEvent.click(screen.getByRole('button', { name: 'Auto orbit' }));
    expect(screen.getByRole('button', { name: 'Pause orbit' })).toHaveAttribute('aria-pressed', 'true');

    fireEvent.click(screen.getByRole('button', { name: 'Reset view' }));
    expect(screen.getByTestId('architecture-scene')).toHaveAttribute('data-reset-revision', '1');
  });
});
