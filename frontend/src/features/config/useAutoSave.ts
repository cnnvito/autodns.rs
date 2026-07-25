import { useCallback, useEffect, useRef, useState, type RefObject } from "react";

import { saveConfig } from "../../shared/api";
import { enqueueSerial } from "../../shared/useSerialSaver";
import type { ConfigDocument, DesktopConfig, DesktopStatus } from "../../shared/types";
import { hasAutoSaveChanges, mergeAutoSaveBaseline, mergeAutoSaveDocument } from "./autosave";

export type AutoSaveState = "idle" | "waiting" | "saving" | "saved" | "blocked" | "error";

const AUTO_SAVE_DEBOUNCE_MS = 700;

type AutoSaveOptions = {
  configDocRef: RefObject<ConfigDocument | null>;
  savedConfigDocRef: RefObject<ConfigDocument | null>;
  /** Advance the persisted baseline after a successful save. */
  commitSavedConfig: (doc: ConfigDocument) => void;
  onStatus: (status: DesktopStatus) => void;
  onSaveError: (err: unknown) => void;
  /** Re-validate the merged snapshot right before persisting it. */
  hasSnapshotErrors: (config: DesktopConfig) => boolean;
  // Reactive inputs that drive (re)scheduling of the debounced save.
  configDoc: ConfigDocument | null;
  savedConfigDoc: ConfigDocument | null;
  autoDirty: boolean;
  eligible: boolean;
  blocked: boolean;
  busy: boolean;
  manualDirty: boolean;
  quitting: boolean;
};

/**
 * Debounced auto-save state machine for the auto-save slice of the config.
 *
 * Owns the shared serial save queue and request-id bookkeeping that manual
 * saves also go through (`enqueueSave`/`beginRequest`), so automatic and
 * manual saves can never interleave or apply stale results.
 */
export function useAutoSave(options: AutoSaveOptions) {
  const { configDoc, savedConfigDoc, autoDirty, eligible, blocked, busy, manualDirty, quitting } = options;
  const [state, setState] = useState<AutoSaveState>("idle");
  const queueRef = useRef<Promise<unknown>>(Promise.resolve());
  const timerRef = useRef<number | undefined>(undefined);
  const revisionRef = useRef(0);
  const taskRef = useRef<Promise<boolean> | null>(null);
  const flushingRef = useRef(false);
  const requestIdRef = useRef(0);
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const cancelScheduled = useCallback(() => {
    revisionRef.current += 1;
    if (timerRef.current !== undefined) {
      window.clearTimeout(timerRef.current);
      timerRef.current = undefined;
    }
  }, []);

  const runNow = useCallback((revision = revisionRef.current): Promise<boolean> => {
    const { configDocRef, savedConfigDocRef } = optionsRef.current;
    const current = configDocRef.current;
    const baseline = savedConfigDocRef.current;
    if (!current || !baseline || revision !== revisionRef.current || !hasAutoSaveChanges(current, baseline)) {
      return Promise.resolve(true);
    }

    const snapshot = mergeAutoSaveDocument(baseline, current);
    if (optionsRef.current.hasSnapshotErrors(snapshot.config)) {
      setState("blocked");
      return Promise.resolve(true);
    }
    const requestId = requestIdRef.current + 1;
    requestIdRef.current = requestId;
    setState("saving");

    const task = enqueueSerial(queueRef, async () => {
      try {
        const result = await saveConfig(snapshot);
        const persistedBaseline = optionsRef.current.savedConfigDocRef.current;
        optionsRef.current.commitSavedConfig(persistedBaseline ? mergeAutoSaveBaseline(persistedBaseline, snapshot) : snapshot);
        if (requestId === requestIdRef.current && revision === revisionRef.current && !optionsRef.current.quitting) {
          optionsRef.current.onStatus(result.status);
          setState("saved");
        }
        return true;
      } catch (err) {
        if (
          requestId === requestIdRef.current
          && (revision === revisionRef.current || flushingRef.current)
        ) {
          setState("error");
          optionsRef.current.onSaveError(err);
        }
        return false;
      }
    });
    taskRef.current = task;
    void task.then(() => {
      if (taskRef.current === task) {
        taskRef.current = null;
      }
    });
    return task;
  }, []);

  /** Flush the scheduled/in-flight auto save before start/stop/restart/quit. */
  const flushPendingSaves = useCallback(async (): Promise<boolean> => {
    flushingRef.current = true;
    try {
      let success = true;
      if (timerRef.current !== undefined) {
        window.clearTimeout(timerRef.current);
        timerRef.current = undefined;
      }
      if (taskRef.current) {
        success = await taskRef.current;
      }
      const { configDocRef, savedConfigDocRef } = optionsRef.current;
      const current = configDocRef.current;
      const baseline = savedConfigDocRef.current;
      if (current && baseline && hasAutoSaveChanges(current, baseline)) {
        success = await runNow(revisionRef.current);
      }
      await queueRef.current;
      return success;
    } finally {
      flushingRef.current = false;
    }
  }, [runNow]);

  useEffect(() => {
    cancelScheduled();

    if (!autoDirty) {
      setState((current) => current === "saved" ? current : "idle");
      return;
    }

    if (!eligible) {
      setState(blocked ? "blocked" : "idle");
      return;
    }

    const revision = revisionRef.current;
    setState("waiting");
    timerRef.current = window.setTimeout(() => {
      timerRef.current = undefined;
      if (!optionsRef.current.configDocRef.current || revision !== revisionRef.current) {
        return;
      }

      void runNow(revision);
    }, AUTO_SAVE_DEBOUNCE_MS);

    return () => {
      if (timerRef.current !== undefined) {
        window.clearTimeout(timerRef.current);
        timerRef.current = undefined;
      }
    };
  }, [autoDirty, blocked, busy, cancelScheduled, configDoc, eligible, manualDirty, quitting, runNow, savedConfigDoc]);

  useEffect(() => () => {
    cancelScheduled();
  }, [cancelScheduled]);

  // Primitives shared with the manual save path.
  const markIdle = useCallback(() => setState("idle"), []);
  const enqueueSave = useCallback(<T,>(task: () => Promise<T>) => enqueueSerial(queueRef, task), []);
  const beginRequest = useCallback(() => {
    requestIdRef.current += 1;
    return requestIdRef.current;
  }, []);
  const isCurrentRequest = useCallback((requestId: number) => requestId === requestIdRef.current, []);

  return { state, markIdle, cancelScheduled, flushPendingSaves, enqueueSave, beginRequest, isCurrentRequest };
}
