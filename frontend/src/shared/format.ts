import type { CommandError, HealthState, LocalizedMessage } from "./types";

export function formatDate(value?: string, locale?: string): string {
  if (!value) {
    return "";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  return date.toLocaleString(locale);
}

type Translate = (key: string, values?: Record<string, unknown>) => string;

export function errorMessage(err: unknown, translate?: Translate): string {
  const commandError = parseCommandError(err);
  if (commandError) {
    return localizedMessageText(commandError, translate);
  }
  if (err instanceof Error) {
    return err.message;
  }
  return String(err);
}

export function localizedMessageText(message: LocalizedMessage | undefined, translate?: Translate): string {
  if (!message) {
    return "";
  }
  if (translate) {
    const key = `errors.${message.code}`;
    const translated = translate(key, message.values);
    if (translated && translated !== key) {
      return translated;
    }
  }
  return interpolateFallback(message.message || message.code, message.values);
}

export function translatedHealthStateLabel(state: HealthState, translate: Translate): string {
  return translate(`common.${state === "healthy" || state === "unhealthy" || state === "unused" ? state : "unknown"}`);
}

function interpolateFallback(message: string, values?: Record<string, unknown>): string {
  if (!values) {
    return message;
  }
  return message.replace(/\{\{?(\w+)\}?\}/g, (match, key: string) => {
    const value = values[key];
    return value === undefined || value === null ? match : String(value);
  });
}

function parseCommandError(err: unknown): CommandError | null {
  if (isCommandError(err)) {
    return err;
  }
  if (typeof err === "string" && err.trim().startsWith("{")) {
    try {
      const parsed = JSON.parse(err) as unknown;
      return isCommandError(parsed) ? parsed : null;
    } catch {
      return null;
    }
  }
  return null;
}

function isCommandError(err: unknown): err is CommandError {
  if (!err || typeof err !== "object") {
    return false;
  }
  const candidate = err as Partial<CommandError>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}
