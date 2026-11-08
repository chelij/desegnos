import type { Element } from './generated/types';

export * from './generated/types';

export type ElementKind = Element['kind'];
export type Viewport = { width: number; height: number };

type WithoutKind<E> = E extends unknown ? Omit<E, 'kind'> : never;
type Intersect<U> = (U extends unknown ? (member: U) => void : never) extends (all: infer I) => void ? I : never;
/** Fields only some element kinds carry (input type, select options, code language and source). */
export type KindFields = Partial<Omit<Intersect<WithoutKind<Element>>, keyof Element>>;
export const kindFields = (element: Element) => element as Element & KindFields;
