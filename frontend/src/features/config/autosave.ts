import type { ConfigDocument, DesktopConfig } from "../../shared/types";

/**
 * These fields are safe to persist after a complete, valid edit. They do not
 * change the listener or the configured upstream/routing topology.
 */
export function autoSaveSnapshot(config: DesktopConfig) {
  const { resolver, cache, healthcheck, log } = config;
  return {
    resolver: {
      timeout: resolver.timeout,
      bootstrapDns: resolver.bootstrapDns,
      defaultProxy: resolver.defaultProxy,
      ipv6Enabled: resolver.ipv6Enabled
    },
    cache,
    healthcheck,
    log
  };
}

/**
 * Everything outside the auto-save slice remains an explicit apply action.
 * Runtime/status fields are intentionally excluded from both comparisons.
 */
function manualSnapshot(config: DesktopConfig) {
  const {
    timeout: _timeout,
    bootstrapDns: _bootstrapDns,
    defaultProxy: _defaultProxy,
    ipv6Enabled: _ipv6Enabled,
    hostStatuses: _hostStatuses,
    routeStatuses: _routeStatuses,
    ...manualResolver
  } = config.resolver;
  const { cache: _cache, healthcheck: _healthcheck, log: _log, ...manualConfig } = config;
  return {
    ...manualConfig,
    resolver: manualResolver
  };
}

function stableString(value: unknown): string {
  return JSON.stringify(value);
}

export function hasAutoSaveChanges(current: ConfigDocument | null, saved: ConfigDocument | null): boolean {
  if (!current || !saved) {
    return false;
  }
  return stableString(autoSaveSnapshot(current.config)) !== stableString(autoSaveSnapshot(saved.config));
}

export function hasManualConfigChanges(current: ConfigDocument | null, saved: ConfigDocument | null): boolean {
  if (!current || !saved) {
    return false;
  }
  return stableString(manualSnapshot(current.config)) !== stableString(manualSnapshot(saved.config));
}

/**
 * Build the document that an automatic save is allowed to apply. The base
 * document supplies every explicit/manual field; only the safe slice comes
 * from the current draft.
 */
export function mergeAutoSaveDocument(base: ConfigDocument, current: ConfigDocument): ConfigDocument {
  return {
    ...base,
    config: {
      ...base.config,
      resolver: {
        ...base.config.resolver,
        timeout: current.config.resolver.timeout,
        bootstrapDns: current.config.resolver.bootstrapDns,
        defaultProxy: current.config.resolver.defaultProxy,
        ipv6Enabled: current.config.resolver.ipv6Enabled
      },
      cache: current.config.cache,
      healthcheck: current.config.healthcheck,
      log: current.config.log
    }
  };
}

/**
 * Advance a persisted baseline after an automatic save without changing any
 * manual draft that may still be present in the UI.
 */
export function mergeAutoSaveBaseline(base: ConfigDocument, persisted: ConfigDocument): ConfigDocument {
  return mergeAutoSaveDocument(base, persisted);
}
