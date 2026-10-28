import type { Element } from '../types';
import { normalizeAngle } from '../numberExpression';
import { elementPadding } from './elementGeometry';

const valuesOf = (element: Element) => {
  const padding = elementPadding(element);
  return {
    fill: element.style.fill,
    strokeColour: element.style.stroke.color,
    strokeWidth: element.style.stroke.width,
    radius: element.style.radius,
    opacity: element.style.opacity,
    shadow: element.style.shadow,
    shadowColour: element.style.shadow.color,
    textStyle: element.style.textStyle,
    textColour: element.style.textStyle.color,
    text: element.text,
    url: element.url,
    layout: element.layout,
    x: element.rect.x,
    y: element.rect.y,
    width: element.rect.width,
    height: element.rect.height,
    rotation: normalizeAngle(element.rotation),
    top: padding.top,
    right: padding.right,
    bottom: padding.bottom,
    left: padding.left,
    fillReference: element.style.references?.fill,
    strokeReference: element.style.references?.stroke,
    textColourReference: element.style.references?.textColour,
    textReference: element.style.references?.text,
    kind: element.kind,
    link: element.link,
  };
};
type Values = ReturnType<typeof valuesOf>;

/** One selection pass; serialise the first object value once and stop comparing mixed fields. */
export function sharedElementValues(elements: Element[]): { [K in keyof Values]: Values[K] | undefined } {
  const first = elements[0];
  if (!first) return {} as { [K in keyof Values]: Values[K] | undefined };
  const shared: { [K in keyof Values]: Values[K] | undefined } = valuesOf(first);
  const keys = Object.keys(shared) as (keyof Values)[];
  const signatures = new Map(keys.map(key => [key, JSON.stringify(shared[key])]));
  for (let index = 1; index < elements.length; index++) {
    const values = valuesOf(elements[index]);
    for (const key of keys) {
      if (shared[key] !== undefined && !Object.is(shared[key], values[key]) && JSON.stringify(values[key]) !== signatures.get(key)) {
        // Each key retains its own value type; assigning undefined is valid for every key.
        shared[key] = undefined;
      }
    }
  }
  return shared;
}
