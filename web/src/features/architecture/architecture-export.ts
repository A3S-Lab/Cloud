const MAX_CANVAS_EDGE = 8_192;
const PREFERRED_EXPORT_SCALE = 2;

export async function exportElementAsPng(element: HTMLElement, filename: string): Promise<void> {
  await waitForDocumentFonts();

  const bounds = element.getBoundingClientRect();
  const width = Math.ceil(Math.max(bounds.width, element.scrollWidth));
  const height = Math.ceil(Math.max(bounds.height, element.scrollHeight));
  if (width <= 0 || height <= 0) {
    throw new Error('The architecture diagram has no exportable area.');
  }

  const clone = element.cloneNode(true) as HTMLElement;
  inlineComputedStyles(element, clone);
  clone.setAttribute('xmlns', 'http://www.w3.org/1999/xhtml');
  clone.style.width = `${width}px`;
  clone.style.height = `${height}px`;
  clone.style.margin = '0';
  clone.style.animation = 'none';
  clone.style.transition = 'none';

  const serialized = new XMLSerializer().serializeToString(clone);
  const svg = [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}"`,
    ` viewBox="0 0 ${width} ${height}">`,
    `<foreignObject width="100%" height="100%">${serialized}</foreignObject>`,
    '</svg>',
  ].join('');
  const svgUrl = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml;charset=utf-8' }));

  let image: HTMLImageElement;
  try {
    image = await loadImage(svgUrl);
  } finally {
    URL.revokeObjectURL(svgUrl);
  }

  const scale = Math.min(PREFERRED_EXPORT_SCALE, MAX_CANVAS_EDGE / width, MAX_CANVAS_EDGE / height);
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(width * scale));
  canvas.height = Math.max(1, Math.round(height * scale));
  const context = canvas.getContext('2d');
  if (!context) {
    throw new Error('This browser cannot create the PNG rendering context.');
  }
  context.setTransform(scale, 0, 0, scale, 0, 0);
  context.drawImage(image, 0, 0, width, height);

  const png = await canvasBlob(canvas);
  downloadBlob(png, filename);
}

function inlineComputedStyles(source: Element, target: Element): void {
  const computed = window.getComputedStyle(source);
  const targetElement = target as HTMLElement;
  for (let index = 0; index < computed.length; index += 1) {
    const property = computed.item(index);
    targetElement.style.setProperty(
      property,
      computed.getPropertyValue(property),
      computed.getPropertyPriority(property)
    );
  }
  targetElement.style.animation = 'none';
  targetElement.style.transition = 'none';

  for (let index = 0; index < source.children.length; index += 1) {
    const sourceChild = source.children.item(index);
    const targetChild = target.children.item(index);
    if (sourceChild && targetChild) inlineComputedStyles(sourceChild, targetChild);
  }
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error('The browser could not render the architecture SVG.'));
    image.src = url;
  });
}

function canvasBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error('The browser could not encode the architecture PNG.'));
    }, 'image/png');
  });
}

function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  link.hidden = true;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

async function waitForDocumentFonts(): Promise<void> {
  if (document.fonts) await document.fonts.ready;
}
