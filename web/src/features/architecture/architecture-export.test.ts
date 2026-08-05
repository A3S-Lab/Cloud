import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { exportElementAsPng } from './architecture-export';

let createObjectUrlDescriptor: PropertyDescriptor | undefined;
let revokeObjectUrlDescriptor: PropertyDescriptor | undefined;

beforeEach(() => {
  document.body.innerHTML = '';
  createObjectUrlDescriptor = Object.getOwnPropertyDescriptor(URL, 'createObjectURL');
  revokeObjectUrlDescriptor = Object.getOwnPropertyDescriptor(URL, 'revokeObjectURL');
});

afterEach(() => {
  restoreUrlProperty('createObjectURL', createObjectUrlDescriptor);
  restoreUrlProperty('revokeObjectURL', revokeObjectUrlDescriptor);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('exportElementAsPng', () => {
  it('serializes the live element and downloads a scaled PNG', async () => {
    const element = document.createElement('div');
    element.innerHTML = '<section><strong>Sole Harness</strong></section>';
    document.body.append(element);
    vi.spyOn(element, 'getBoundingClientRect').mockReturnValue({
      width: 1_200,
      height: 700,
      top: 0,
      right: 1_200,
      bottom: 700,
      left: 0,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });
    Object.defineProperty(element, 'scrollWidth', { configurable: true, value: 1_200 });
    Object.defineProperty(element, 'scrollHeight', { configurable: true, value: 700 });

    const createObjectURL = vi
      .fn<(blob: Blob) => string>()
      .mockReturnValueOnce('blob:a3s-architecture-svg')
      .mockReturnValueOnce('blob:a3s-architecture-png');
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: createObjectURL });
    Object.defineProperty(URL, 'revokeObjectURL', { configurable: true, value: revokeObjectURL });

    class ImmediateImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;

      set src(_value: string) {
        queueMicrotask(() => this.onload?.());
      }
    }
    vi.stubGlobal('Image', ImmediateImage);

    const context = {
      setTransform: vi.fn(),
      drawImage: vi.fn(),
    };
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(
      context as unknown as CanvasRenderingContext2D
    );
    vi.spyOn(HTMLCanvasElement.prototype, 'toBlob').mockImplementation((callback) => {
      callback(new Blob(['png'], { type: 'image/png' }));
    });
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);

    await exportElementAsPng(element, 'architecture.png');

    const svgBlob = createObjectURL.mock.calls[0]?.[0];
    expect(svgBlob?.type).toBe('image/svg+xml;charset=utf-8');
    expect(await readBlob(svgBlob)).toContain('Sole Harness');
    expect(createObjectURL.mock.calls[1]?.[0].type).toBe('image/png');
    expect(context.setTransform).toHaveBeenCalledWith(2, 0, 0, 2, 0, 0);
    expect(context.drawImage).toHaveBeenCalledOnce();
    expect(click).toHaveBeenCalledOnce();
    expect(revokeObjectURL.mock.calls).toEqual([
      ['blob:a3s-architecture-svg'],
      ['blob:a3s-architecture-png'],
    ]);
  });
});

function readBlob(blob: Blob | undefined): Promise<string> {
  if (!blob) throw new Error('SVG blob is missing');
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error('Could not read SVG blob'));
    reader.readAsText(blob);
  });
}

function restoreUrlProperty(name: 'createObjectURL' | 'revokeObjectURL', descriptor?: PropertyDescriptor) {
  if (descriptor) Object.defineProperty(URL, name, descriptor);
  else Reflect.deleteProperty(URL, name);
}
