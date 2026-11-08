// Generated from Rust by `npm run generate:contract`. Do not edit.

export const GRID_PIXEL_MIN = 1;
export const GRID_PIXEL_MAX = 512;
export const GRID_PERCENT_MIN = 0.25;
export const GRID_PERCENT_MAX = 100;
export const MAX_LAYER_LABEL_CHARS = 80;
export const MAX_FONT_WEIGHT = 1000;
export const MAX_GRID_COLUMNS = 64;
export const MAX_ATTACHMENT_TEXT_CHARS = 65536;
export const MAX_EDITOR_COORDINATE = 1000000;
export const MAX_PASTED_IMAGE_WIDTH = 480;
export const PASTED_NOTE_WIDTH = 240;
export const PASTED_NOTE_HEIGHT = 140;
export const SWATCH_SIZE = 120;
export const MAX_IMPORT_PAGES = 30;
export const MAX_REQUEST_FILE_BYTES = 33554432;
export const MAX_PROMPT_CHARS = 16384;
export const MAX_NAME_CHARS = 120;
export const MAX_BRANCH_CHARS = 60;
export const MAX_NAMED_VIEW_NAME_CHARS = 80;
export const MAX_HTTP_URL_BYTES = 2048;
export const MAX_ATTACHMENTS = 32;
export const MAX_VIEWPORT_SIDE = 100000;
export const DEFAULT_GRID_SPACING = 8;
export const TEMPLATE_GAP = 160;
export const TEXT_SIZE_RANGE = [1.0, 512.0] as const;
export const IMAGE_MIME_TYPES: readonly string[] = ["image/png","image/jpeg","image/webp","image/gif"];
export const RESERVED_ROUTE_NAMES: readonly string[] = ["assets","src","node_modules","con","prn","aux","nul","com1","com2","com3","com4","com5","com6","com7","com8","com9","lpt1","lpt2","lpt3","lpt4","lpt5","lpt6","lpt7","lpt8","lpt9"];
export const SIZE_LABELS = [
  { min: 0, label: "Phone" },
  { min: 700, label: "Tablet" },
  { min: 1024, label: "Desktop" },
] as const;
export const ELEMENT_DEFAULTS = {
  box: { label: "Box", name: "Rectangle", text: "", width: 240, height: 160 },
  group: { label: "Group", name: "Group", text: "", width: 240, height: 160 },
  text: { label: "Text", name: "Text", text: "Text", width: 240, height: 160 },
  ellipse: { label: "Ellipse", name: "Ellipse", text: "", width: 160, height: 120 },
  line: { label: "Line", name: "Line", text: "", width: 160, height: 0 },
  image: { label: "Image", name: "Image", text: "", width: 320, height: 200 },
  button: { label: "Button", name: "Button", text: "Button", width: 160, height: 44 },
  link: { label: "Link", name: "Link", text: "Link", width: 120, height: 28 },
  input: { label: "Input", name: "Input", text: "Input", width: 240, height: 40 },
  textarea: { label: "Textarea", name: "Textarea", text: "Textarea", width: 240, height: 100 },
  select: { label: "Select", name: "Select", text: "Option 1", width: 240, height: 40 },
  checkbox: { label: "Checkbox", name: "Checkbox", text: "Checkbox", width: 160, height: 24 },
  radio: { label: "Radio", name: "Radio", text: "Radio", width: 160, height: 24 },
  video: { label: "Video", name: "Video", text: "", width: 320, height: 180 },
  audio: { label: "Audio", name: "Audio", text: "", width: 320, height: 64 },
  embed: { label: "Embed", name: "Embed", text: "", width: 320, height: 180 },
  code: { label: "Code", name: "Code", text: "", width: 360, height: 160 },
} as const;
export const TEMPLATE_DEFAULTS = {
  "blank": { output: "webpage", kind: "page", width: 1440, height: 900 },
  "website": { output: "webpage", kind: "page", width: 1440, height: 900 },
  "appScreen": { output: "webpage", kind: "screen", width: 390, height: 844 },
  "slideDeck": { output: "slide", kind: "screen", width: 1920, height: 1080 },
  "socialImage": { output: "image", kind: "screen", width: 1080, height: 1080 },
} as const;

export const SIZE_PRESETS = {
  phone: { name: "Phone", width: 390, height: 844 },
  tablet: { name: "Tablet", width: 820, height: 1180 },
  desktop: { name: "Desktop", width: 1440, height: 900 },
} as const;
