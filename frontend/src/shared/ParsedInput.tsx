import { Input, type InputProps } from "antd";
import { useEffect, useState } from "react";

import { FieldWithError } from "./FieldWithError";

type ParsedInputProps<T> = Omit<InputProps, "value" | "onChange" | "onBlur" | "onPressEnter" | "status"> & {
  /** Formatted external value shown while the field is not being edited. */
  value: string;
  /** Parse a raw draft into a config patch; null marks the draft invalid. */
  parse: (raw: string) => T | null;
  /** Applied for every valid draft (live) and again when editing finishes. */
  onApply: (patch: T) => void;
  /** Error shown while the current draft fails to parse. */
  invalidText: string;
  /** Validation error coming from the config itself. */
  externalError?: string;
};

/**
 * Free-form input backed by a parsed config value. While focused it keeps a
 * local draft (so partial input is never clobbered by re-renders), applies
 * every parseable draft immediately, and on blur snaps back to the formatted
 * external value. The draft resets automatically when the row's value changes
 * from outside (e.g. rows added, removed, or reordered).
 */
export function ParsedInput<T>({ value, parse, onApply, invalidText, externalError, ...props }: ParsedInputProps<T>) {
  const [draft, setDraft] = useState<string | null>(null);
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    if (!focused) {
      setDraft(null);
    }
  }, [focused, value]);

  const shown = draft ?? value;
  const error = draft !== null && !parse(draft) ? invalidText : externalError;

  function commit() {
    setFocused(false);
    if (draft !== null) {
      const patch = parse(draft);
      if (patch) {
        onApply(patch);
      }
      setDraft(null);
    }
  }

  return (
    <FieldWithError error={error}>
      <Input
        {...props}
        value={shown}
        status={error ? "error" : undefined}
        onFocus={() => setFocused(true)}
        onChange={(event) => {
          const next = event.target.value;
          setDraft(next);
          const patch = parse(next);
          if (patch) {
            onApply(patch);
          }
        }}
        onBlur={commit}
        onPressEnter={(event) => event.currentTarget.blur()}
      />
    </FieldWithError>
  );
}
