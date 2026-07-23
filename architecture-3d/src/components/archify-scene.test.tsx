import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ArchifyScene } from './archify-scene';

afterEach(() => {
  vi.useRealTimers();
});

describe('ArchifyScene', () => {
  it('bridges Archify replaceState selections to the shared inspector', () => {
    vi.useFakeTimers();
    const onSelect = vi.fn();
    render(<ArchifyScene onClearSelection={vi.fn()} onSelect={onSelect} />);

    const iframe = screen.getByTitle<HTMLIFrameElement>('A3S Cloud 2D architecture powered by Archify');
    const childWindow = iframe.contentWindow;
    expect(childWindow).not.toBeNull();
    if (!childWindow) return;

    childWindow.document.open();
    childWindow.document.write('<div class="diagram-container"><svg data-preset="architecture"></svg></div>');
    childWindow.document.close();
    fireEvent.load(iframe);

    act(() => {
      childWindow.history.replaceState(null, '', '#focus=power');
      vi.advanceTimersByTime(100);
    });

    expect(onSelect).toHaveBeenCalledWith({ kind: 'node', id: 'power' });
  });
});
