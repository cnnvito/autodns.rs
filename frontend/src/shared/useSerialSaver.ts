import { useCallback, useRef, useState } from "react";

export function enqueueSerial<T>(queue: { current: Promise<unknown> }, task: () => Promise<T>): Promise<T> {
  const next = queue.current.catch(() => undefined).then(task);
  queue.current = next.catch(() => undefined);
  return next;
}

type SerialSaverOptions<Value, Input> = {
  /** Persist one submission; the resolved value becomes the new baseline. */
  save: (input: Input) => Promise<Value>;
  /** Notify about a failed save (called only for the most recent submission). */
  onError: (err: unknown) => void;
};

/**
 * State that is edited optimistically and persisted through a serial queue.
 *
 * Each submit updates the UI immediately, then enqueues the save. Outdated
 * submissions (superseded by a newer one) neither update state nor report
 * errors; a failed latest submission rolls the value back to the last
 * persisted baseline. `sync` installs an externally loaded value as the new
 * baseline, `flush` awaits the queue (e.g. before quitting).
 */
export function useSerialSaver<Value, Input>({ save, onError }: SerialSaverOptions<Value, Input>) {
  const [value, setValue] = useState<Value | null>(null);
  const valueRef = useRef<Value | null>(null);
  const persistedRef = useRef<Value | null>(null);
  const queueRef = useRef<Promise<unknown>>(Promise.resolve());
  const revisionRef = useRef(0);
  const pendingRef = useRef(0);
  const failedRef = useRef(false);
  const optionsRef = useRef({ save, onError });
  optionsRef.current = { save, onError };

  const apply = useCallback((next: Value | null) => {
    valueRef.current = next;
    setValue(next);
  }, []);

  /** Install an externally loaded value as both current value and baseline. */
  const sync = useCallback((next: Value) => {
    failedRef.current = false;
    persistedRef.current = next;
    apply(next);
  }, [apply]);

  const submit = useCallback((optimistic: Value | null, input: Input): Promise<void> => {
    const revision = revisionRef.current + 1;
    revisionRef.current = revision;
    pendingRef.current += 1;
    if (optimistic !== null) {
      apply(optimistic);
    }

    return enqueueSerial(queueRef, async () => {
      try {
        const saved = await optionsRef.current.save(input);
        persistedRef.current = saved;
        if (revision === revisionRef.current) {
          failedRef.current = false;
          apply(saved);
        }
      } catch (err) {
        if (revision === revisionRef.current) {
          failedRef.current = true;
          apply(persistedRef.current);
          optionsRef.current.onError(err);
        }
      } finally {
        pendingRef.current = Math.max(0, pendingRef.current - 1);
      }
    });
  }, [apply]);

  const flush = useCallback(() => queueRef.current, []);
  const current = useCallback(() => valueRef.current, []);
  const hasPending = useCallback(() => pendingRef.current > 0, []);
  const lastFailed = useCallback(() => failedRef.current, []);

  return { value, current, submit, sync, flush, hasPending, lastFailed };
}
