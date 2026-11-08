import type { NativeErrorKind, NativeFailure, ProviderError, ProviderId } from './types';

export type ErrorValue = string | Partial<NativeFailure> & { message: string; providerError?: ProviderError | null; provider?: ProviderId };

export function errorMessage(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) return String(error.message ?? '');
  return String(error ?? '');
}

export function errorValue(error: unknown): ErrorValue {
  return error && typeof error === 'object' && 'message' in error
    ? { ...error, message: errorMessage(error) } : errorMessage(error);
}

export function nativeErrorKind(error: unknown): NativeErrorKind | undefined {
  return error && typeof error === 'object' && 'kind' in error ? error.kind as NativeErrorKind : undefined;
}
