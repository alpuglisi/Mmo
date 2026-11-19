// Small line icons for the map toolbar and legend (16 px grid, 1.75 px
// round strokes in currentColor, after client/src/world/STYLE.md). Built as
// SVG elements, no assets.

const PATHS = {
  follow: 'M8 1.5v2.5M8 12v2.5M1.5 8H4M12 8h2.5M8 4.5a3.5 3.5 0 1 0 0 7a3.5 3.5 0 1 0 0-7zM8 7.2v1.6',
  plus: 'M8 3v10M3 8h10',
  minus: 'M3 8h10',
  migrate: 'M3 13L12.5 3.5M6.5 3.5h6v6',
  clear: 'M8 1.8a6.2 6.2 0 1 0 0 12.4a6.2 6.2 0 1 0 0-12.4zM5.8 5.8l4.4 4.4M10.2 5.8l-4.4 4.4',
  territory: 'M8 2a6 6 0 1 0 0 12a6 6 0 1 0 0-12zM8 6.6a1.4 1.4 0 1 0 0 2.8a1.4 1.4 0 1 0 0-2.8z',
  rally: 'M4 14.5V2M4 2.8h8l-2 2.7 2 2.7H4',
  split: 'M8 14.5V9M8 9L3.5 3.5M8 9l4.5-5.5M3.5 3.5v3M3.5 3.5h3M12.5 3.5v3M12.5 3.5h-3',
  pin: 'M8 10.5v4M5 2h6l-1 4.2 2.2 2.8H3.8L6 6.2z',
  eye: 'M1.5 8C3.5 4.5 5.6 3.5 8 3.5S12.5 4.5 14.5 8C12.5 11.5 10.4 12.5 8 12.5S3.5 11.5 1.5 8zM8 6a2 2 0 1 0 0 4a2 2 0 1 0 0-4z',
  legend: 'M3 4h1M6.5 4H13M3 8h1M6.5 8H13M3 12h1M6.5 12H13',
  more: 'M3.5 8h.01M8 8h.01M12.5 8h.01',
  food: 'M3 13.5C3 7 7 3 13.5 2.5C13 9 9 13 3 13.5zM3 13.5L9 7.5',
} as const;

export type IconName = keyof typeof PATHS;

export function icon(name: IconName, cls = 'icon'): SVGSVGElement {
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('class', cls);
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', name === 'more' ? '2.6' : '1.75');
  svg.setAttribute('stroke-linecap', 'round');
  svg.setAttribute('stroke-linejoin', 'round');
  const p = document.createElementNS(ns, 'path');
  p.setAttribute('d', PATHS[name]);
  svg.appendChild(p);
  return svg;
}
