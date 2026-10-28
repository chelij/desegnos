/**
 * Small arithmetic evaluator for compact numeric fields, with no `eval` and no `Function`.
 *
 * Supported input:
 * - Absolute expressions: `120*2`, `100/3`, `(10+5)*2`, `-8`, `--8` (double negative).
 * - Relative entries against the field's current value: `+16`, `*2`, `/2`, `+-8` (subtract 8).
 *   A leading `+`, `*` or `/` is relative (Figma's rule); a leading `-` is an absolute negative
 *   number, so `-8` never means "subtract 8" — write `+-8` for that.
 * - Trailing CSS units on numbers are ignored: `24px`, `50%`, `12.5rem`, `24px + 8px`.
 *
 * Anything else returns `null`, and the field keeps its previous value.
 */

const UNITS = [
  'vmin', 'vmax', 'rem', 'deg', 'rad', 'turn', 'px', 'pt', 'pc', 'cm', 'mm', 'in', 'ch', 'ex', 'fr', 'ms', 'em', 's', 'vh', 'vw', '%',
].join('|');

/** `24px` -> `24`, `24px + 8px` -> `24 + 8`; only strips a unit directly after a number. */
const UNIT_SUFFIX = new RegExp(`((?:\\d+(?:\\.\\d*)?|\\.\\d+))\\s*(?:${UNITS})(?=\\s*(?:[-+*/)]|$))`, 'gi');

function stripUnits(input: string): string {
  return input.replace(UNIT_SUFFIX, '$1');
}

type Cursor = { text: string; index: number };

function skipSpace(cursor: Cursor) {
  while (cursor.index < cursor.text.length && /\s/.test(cursor.text[cursor.index])) cursor.index++;
}

function readExpression(cursor: Cursor): number | null {
  let left = readTerm(cursor);
  if (left === null) return null;
  for (;;) {
    skipSpace(cursor);
    const operator = cursor.text[cursor.index];
    if (operator !== '+' && operator !== '-') return left;
    cursor.index++;
    const right = readTerm(cursor);
    if (right === null) return null;
    left = operator === '+' ? left + right : left - right;
  }
}

function readTerm(cursor: Cursor): number | null {
  let left = readUnary(cursor);
  if (left === null) return null;
  for (;;) {
    skipSpace(cursor);
    const operator = cursor.text[cursor.index];
    if (operator !== '*' && operator !== '/') return left;
    cursor.index++;
    const right = readUnary(cursor);
    if (right === null || (operator === '/' && right === 0)) return null;
    left = operator === '*' ? left * right : left / right;
  }
}

function readUnary(cursor: Cursor): number | null {
  skipSpace(cursor);
  const sign = cursor.text[cursor.index];
  if (sign === '+' || sign === '-') {
    cursor.index++;
    const value = readUnary(cursor);
    if (value === null) return null;
    return sign === '-' ? -value : value;
  }
  return readPrimary(cursor);
}

function readPrimary(cursor: Cursor): number | null {
  skipSpace(cursor);
  if (cursor.text[cursor.index] === '(') {
    cursor.index++;
    const value = readExpression(cursor);
    if (value === null) return null;
    skipSpace(cursor);
    if (cursor.text[cursor.index] !== ')') return null;
    cursor.index++;
    return value;
  }
  const match = /^(?:\d+(?:\.\d*)?|\.\d+)/.exec(cursor.text.slice(cursor.index));
  if (!match) return null;
  cursor.index += match[0].length;
  return Number(match[0]);
}

/** Round to at most two decimals and normalise negative zero. */
export function roundNumber(value: number): number {
  const rounded = Math.round(value * 100) / 100;
  return Object.is(rounded, -0) ? 0 : rounded;
}

export function clampNumber(value: number, min = Number.NEGATIVE_INFINITY, max = Number.POSITIVE_INFINITY): number {
  return Math.min(max, Math.max(min, value));
}

/** An angle in the range -180..180, so 99999° becomes -81°. */
export function normalizeAngle(value: number): number {
  const wrapped = ((value % 360) + 360) % 360;
  return roundNumber(wrapped > 180 ? wrapped - 360 : wrapped);
}

/**
 * Evaluate a field draft. `current` enables relative entries; pass null when the field has no
 * usable previous value. Returns the rounded result, or null when the input is not arithmetic.
 */
export function evaluateNumberExpression(input: string, current: number | null = null): number | null {
  let text = stripUnits(input.trim());
  if (!text) return null;

  const usableCurrent = current !== null && Number.isFinite(current) ? current : null;
  const relative = usableCurrent !== null && /^[+*/]/.test(text) ? text[0] : null;
  if (relative) {
    text = text.slice(1);
    if (!text.trim()) return null;
  } else if (usableCurrent === null && /^[*/]/.test(text)) {
    return null;
  }

  const cursor: Cursor = { text, index: 0 };
  const parsed = readExpression(cursor);
  if (parsed === null) return null;
  skipSpace(cursor);
  if (cursor.index !== text.length || !Number.isFinite(parsed)) return null;

  let result = parsed;
  if (relative === '+') result = usableCurrent! + parsed;
  else if (relative === '*') result = usableCurrent! * parsed;
  else if (relative === '/') {
    if (parsed === 0) return null;
    result = usableCurrent! / parsed;
  }
  if (!Number.isFinite(result)) return null;
  return roundNumber(result);
}

/**
 * Turn a draft into the string a field should commit, or null when the draft is invalid.
 * An empty draft resolves to '' so optional numeric fields can still be cleared.
 */
export function resolveNumberField(
  input: string,
  current: string | number | null | undefined,
  min?: number,
  max?: number,
): string | null {
  const trimmed = input.trim();
  if (!trimmed) return '';
  const previous = typeof current === 'number' ? current : Number(String(current ?? '').trim());
  const base = String(current ?? '').trim() === '' || !Number.isFinite(previous) ? null : previous;
  const value = evaluateNumberExpression(trimmed, base);
  if (value === null) return null;
  return String(roundNumber(clampNumber(value, min, max)));
}
