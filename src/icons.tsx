import type { ReactNode, SVGProps } from 'react';

// One icon style for the whole editor: inline SVG, 16px box, 1.5px currentColor stroke.
export type IconName =
  | 'close' | 'plus' | 'minus' | 'check' | 'chevron-down' | 'chevron-right' | 'more'
  | 'refresh' | 'paperclip' | 'undo' | 'trash' | 'arrow-left' | 'arrow-right'
  | 'reload' | 'external' | 'sun' | 'moon' | 'layers' | 'copy' | 'paste'
  | 'font-smaller' | 'font-larger' | 'padding-less' | 'padding-more'
  | 'align-left' | 'align-center' | 'align-right' | 'text-color' | 'fill-color'
  | 'link' | 'text' | 'info' | 'properties' | 'arrange-front' | 'arrange-forward'
  | 'arrange-backward' | 'arrange-back' | 'monitor' | 'tablet' | 'phone' | 'canvas'
  | 'rectangle' | 'ellipse' | 'line' | 'image' | 'video' | 'audio' | 'code' | 'note' | 'group'
  | 'eye' | 'eye-off' | 'lock' | 'unlock' | 'insert' | 'history' | 'pin' | 'home' | 'folder';

const paths: Record<IconName, ReactNode> = {
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  plus: <path d="M8 3.5v9M3.5 8h9" />,
  minus: <path d="M3.5 8h9" />,
  check: <path d="M3.5 8.5l3 3 6-7" />,
  'chevron-down': <path d="M4 6l4 4 4-4" />,
  'chevron-right': <path d="M6 4l4 4-4 4" />,
  more: <g fill="currentColor" stroke="none"><circle cx="3.5" cy="8" r="1.15" /><circle cx="8" cy="8" r="1.15" /><circle cx="12.5" cy="8" r="1.15" /></g>,
  refresh: <><path d="M13 8a5 5 0 1 1-1.6-3.7" /><path d="M13 2.8v3.4h-3.4" /></>,
  paperclip: <path d="M9.6 4.4 4.9 9.1a2.1 2.1 0 0 0 3 3l4.5-4.5a3.3 3.3 0 0 0-4.7-4.7L3.3 7.3a4.4 4.4 0 0 0 6.2 6.2l3-3" />,
  undo: <><path d="M3 6h6.2a4 4 0 0 1 0 8H7.5" /><path d="M6 3 3 6l3 3" /></>,
  trash: <><path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5" /><path d="M6.8 7v3.5M9.2 7v3.5" /></>,
  'arrow-left': <><path d="M12.5 8H3.5" /><path d="M7 4 3 8l4 4" /></>,
  'arrow-right': <><path d="M3.5 8h9" /><path d="M9 4l4 4-4 4" /></>,
  reload: <><path d="M13 8a5 5 0 1 1-1.6-3.7" /><path d="M13 2.8v3.4h-3.4" /></>,
  external: <><path d="M9.5 3H13v3.5" /><path d="M13 3 7.5 8.5" /><path d="M11.5 9.5V13H3V4.5h3.5" /></>,
  sun: <><circle cx="8" cy="8" r="3" /><path d="M8 1.5v1.7M8 12.8v1.7M1.5 8h1.7M12.8 8h1.7M3.4 3.4l1.2 1.2M11.4 11.4l1.2 1.2M3.4 12.6l1.2-1.2M11.4 4.6l1.2-1.2" /></>,
  moon: <path d="M13 9.6A5.7 5.7 0 0 1 6.4 3a5.7 5.7 0 1 0 6.6 6.6Z" />,
  layers: <><path d="M2 5.2 8 2l6 3.2-6 3.2Z" /><path d="M2 8.2 8 11.4l6-3.2M2 11.2 8 14.4l6-3.2" /></>,
  history: <><circle cx="8" cy="8" r="5.75" /><path d="M8 4.75V8l2.25 1.6" /></>,
  pin: <><path d="M6.2 2.5h3.6l-.5 3 2.2 2.2H4.5l2.2-2.2Z" /><path d="M8 7.7v5.8" /></>,
  copy: <><path d="M5.5 5.5h8v8h-8Z" /><path d="M10.5 5.5v-3h-8v8h3" /></>,
  paste: <><path d="M6 3.5H4.5v10h7v-10H10" /><path d="M5.5 2.5h5v2h-5Z" /><path d="M6.2 7h3.6M6.2 9.8h2.6" /></>,
  'font-smaller': <><path d="M1.8 12.2 4.8 4l3 8.2M2.9 9.3h3.8" /><path d="M11 12h3.2" /></>,
  'font-larger': <><path d="M1.8 12.2 4.8 4l3 8.2M2.9 9.3h3.8" /><path d="M11 12h3.2M12.6 10.4v3.2" /></>,
  'padding-less': <><path d="M2.8 2.8h10.4v10.4H2.8Z" /><path d="M6.2 8h3.6" /></>,
  'padding-more': <><path d="M2.8 2.8h10.4v10.4H2.8Z" /><path d="M6.2 8h3.6M8 6.2v3.6" /></>,
  'align-left': <path d="M2.5 4h11M2.5 8h7M2.5 12h9" />,
  'align-center': <path d="M2.5 4h11M4.5 8h7M3.5 12h9" />,
  'align-right': <path d="M2.5 4h11M6.5 8h7M4.5 12h9" />,
  'text-color': <><path d="M3.2 11 7 3l3.8 8M4.6 8h4.8" /><path d="M2.5 13.6h11" /></>,
  'fill-color': <><path d="M8 2.5 12.5 7a2.2 2.2 0 0 1 0 3.1l-2.4 2.4a2.2 2.2 0 0 1-3.1 0L4 9.6a2.2 2.2 0 0 1 0-3.1Z" /><path d="M4.3 8.5h7.4" /></>,
  link: <><path d="M6.6 9.4a2.8 2.8 0 0 0 4 0l1.7-1.7a2.8 2.8 0 1 0-4-4L7.2 4.8" /><path d="M9.4 6.6a2.8 2.8 0 0 0-4 0L3.7 8.3a2.8 2.8 0 1 0 4 4l1.1-1.1" /></>,
  text: <path d="M3 3.5h10M8 3.5v9M5.5 12.5h5" />,
  properties: <><path d="M2.5 5h4M9.5 5h4M2.5 11h4M9.5 11h4" /><circle cx="8" cy="5" r="1.4" /><circle cx="8" cy="11" r="1.4" /></>,
  info: <><circle cx="8" cy="8" r="5.6" /><path d="M8 7.2v3.6M8 5.2v.2" /></>,
  'arrange-front': <><path d="M2 4.5 8 1.8l6 2.7-6 2.7Z" /><path d="M11.8 8.6V13M9.4 10.6l2.4-2.4 2.4 2.4" /></>,
  'arrange-forward': <><path d="M2 6.5 8 3.8l6 2.7-6 2.7Z" /><path d="M11.8 7.6V12M9.4 9.6l2.4-2.4L14.2 9.6" /></>,
  'arrange-backward': <><path d="M2 6.5 8 3.8l6 2.7-6 2.7Z" /><path d="M11.8 12V7.6M9.4 9.6l2.4 2.4 2.4-2.4" /></>,
  'arrange-back': <><path d="M2 4.5 8 1.8l6 2.7-6 2.7Z" /><path d="M11.8 13V8.6M9.4 10.6l2.4 2.4 2.4-2.4" /></>,
  monitor: <><rect x="1.8" y="3" width="12.4" height="8" rx="1" /><path d="M5.5 13.5h5M8 11v2.5" /></>,
  canvas: <><rect x="1.8" y="2.8" width="5" height="10.4" rx="1" /><rect x="9.2" y="2.8" width="5" height="10.4" rx="1" /></>,
  tablet: <><rect x="3" y="1.8" width="10" height="12.4" rx="1.2" /><path d="M7 12.2h2" /></>,
  phone: <><rect x="4.3" y="1.8" width="7.4" height="12.4" rx="1.4" /><path d="M7 12.2h2" /></>,
  rectangle: <rect x="2.2" y="3.2" width="11.6" height="9.6" rx="1" />,
  ellipse: <ellipse cx="8" cy="8" rx="5.8" ry="4.8" />,
  line: <path d="M2.5 13.2 13.5 2.8" />,
  image: <><rect x="2.2" y="3" width="11.6" height="10" rx="1" /><circle cx="6" cy="6.4" r="1.1" /><path d="M3 12 6.8 8.2l2.2 2.2 1.9-1.9 2 2.5" /></>,
  video: <><rect x="1.8" y="3.6" width="9.4" height="8.8" rx="1" /><path d="M11.2 7.2 14 5.2v5.6l-2.8-2Z" /></>,
  audio: <><path d="M3 6.2h2.2L8.6 3v10L5.2 9.8H3Z" /><path d="M10.6 5.8a3 3 0 0 1 0 4.4M12.4 4.2a5.2 5.2 0 0 1 0 7.6" /></>,
  code: <><path d="M6 4.2 2.4 8 6 11.8M10 4.2 13.6 8 10 11.8" /></>,
  note: <><path d="M2.6 2.8h10.8v7.6l-3 3H2.6Z" /><path d="M13.4 10.4h-3v3" /></>,
  group: <><rect x="2" y="3.2" width="8.2" height="8.2" rx="1" /><path d="M5.8 12.8h8.2V4.6" /></>,
  eye: <><path d="M1.8 8S4.1 4.2 8 4.2 14.2 8 14.2 8 11.9 11.8 8 11.8 1.8 8 1.8 8Z" /><circle cx="8" cy="8" r="1.8" /></>,
  'eye-off': <><path d="M2.4 2.4 13.6 13.6" /><path d="M6.1 4.6A7 7 0 0 1 8 4.2c3.9 0 6.2 3.8 6.2 3.8a12 12 0 0 1-2.1 2.6M4.2 5.7A12.8 12.8 0 0 0 1.8 8S4.1 11.8 8 11.8a6.9 6.9 0 0 0 2.2-.4" /></>,
  lock: <><rect x="3.4" y="7" width="9.2" height="6.2" rx="1" /><path d="M5.6 7V5.4a2.4 2.4 0 0 1 4.8 0V7" /></>,
  unlock: <><rect x="3.4" y="7" width="9.2" height="6.2" rx="1" /><path d="M5.6 7V5.4a2.4 2.4 0 0 1 4.6-.8" /></>,
  insert: <><path d="M8 2.6v10.8M2.6 8h10.8" /><circle cx="8" cy="8" r="6.2" /></>,
  home: <><path d="M2.2 7.4 8 2.6l5.8 4.8" /><path d="M3.8 6.6v6.8h8.4V6.6" /><path d="M6.4 13.4V9.6h3.2v3.8" /></>,
  folder: <path d="M2 4.4h4.1l1.4 1.8H14v6.4H2Z" />,
};

export function Icon({ name, size, className = '', ...rest }: { name: IconName; size?: number } & Omit<SVGProps<SVGSVGElement>, 'name'>) {
  return <svg className={`icon${className ? ` ${className}` : ''}`} width={size ?? 16} height={size ?? 16} viewBox="0 0 16 16"
    fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round"
    aria-hidden="true" focusable="false" {...rest}>{paths[name]}</svg>;
}
