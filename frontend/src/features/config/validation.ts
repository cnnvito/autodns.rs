import { parseHost, parseRoute } from "./transforms";
import type { DesktopConfig, ProxyConfig, UpstreamConfig } from "../../shared/types";

type Translate = (key: string, values?: Record<string, string | number>) => string;

export type ConfigValidation = {
  server: {
    listen?: string;
    path?: string;
    certFile?: string;
    keyFile?: string;
  };
  resolver: {
    timeout?: string;
    bootstrapDns: Record<number, string>;
    upstreams: Record<number, Partial<Record<"name" | "endpoint" | "serverName" | "proxy", string>>>;
    proxies: Record<number, Partial<Record<"name" | "address" | "port", string>>>;
    hosts: Record<number, Partial<Record<"domain" | "ips", string>>>;
    routes: Record<number, Partial<Record<"domain" | "upstreams", string>>>;
  };
  cache: Partial<Record<keyof DesktopConfig["cache"], string>>;
  healthcheck: Partial<Record<keyof DesktopConfig["healthcheck"], string>>;
};

const emptyValidation: ConfigValidation = {
  server: {},
  resolver: {
    bootstrapDns: {},
    upstreams: {},
    proxies: {},
    hosts: {},
    routes: {}
  },
  cache: {},
  healthcheck: {}
};

export function validateDesktopConfig(config: DesktopConfig, t: Translate = defaultTranslate): ConfigValidation {
  const result: ConfigValidation = {
    server: {},
    resolver: {
      bootstrapDns: {},
      upstreams: {},
      proxies: {},
      hosts: {},
      routes: {}
    },
    cache: {},
    healthcheck: {}
  };

  validateServer(config, result, t);
  validateResolver(config, result, t);
  validateCache(config, result, t);
  validateHealthcheck(config, result, t);

  return result;
}

export function hasValidationErrors(validation: ConfigValidation): boolean {
  return flattenValidationMessages(validation).length > 0;
}

export function flattenValidationMessages(validation: ConfigValidation): string[] {
  const messages: string[] = [];
  collect(validation.server, messages);
  collect(validation.resolver.bootstrapDns, messages);
  Object.values(validation.resolver.upstreams).forEach((item) => collect(item, messages));
  Object.values(validation.resolver.proxies).forEach((item) => collect(item, messages));
  Object.values(validation.resolver.hosts).forEach((item) => collect(item, messages));
  Object.values(validation.resolver.routes).forEach((item) => collect(item, messages));
  collect(validation.cache, messages);
  collect(validation.healthcheck, messages);
  return messages;
}

export function emptyConfigValidation(): ConfigValidation {
  return structuredClone(emptyValidation);
}

function validateServer(config: DesktopConfig, result: ConfigValidation, t: Translate) {
  const mode = config.server.mode;
  if (!parseSocketAddress(config.server.listen)) {
    result.server.listen = t("validation.server.listen");
  }
  if (mode === "doh" && !config.server.path.trim().startsWith("/")) {
    result.server.path = t("validation.server.path");
  }
  if ((mode === "doh" || mode === "dot") && !config.server.certFile.trim()) {
    result.server.certFile = t("validation.server.certFile");
  }
  if ((mode === "doh" || mode === "dot") && !config.server.keyFile.trim()) {
    result.server.keyFile = t("validation.server.keyFile");
  }
}

function validateResolver(config: DesktopConfig, result: ConfigValidation, t: Translate) {
  if (!isDuration(config.resolver.timeout)) {
    result.resolver.timeout = t("validation.resolver.timeout");
  }
  config.resolver.bootstrapDns.forEach((server, index) => {
    if (!parseBootstrapDns(server)) {
      result.resolver.bootstrapDns[index] = t("validation.resolver.bootstrapDns");
    }
  });

  const upstreamNames = new Map<string, number>();
  config.resolver.upstreams.forEach((upstream, index) => {
    const errors: ConfigValidation["resolver"]["upstreams"][number] = {};
    const name = upstream.name.trim();
    if (!name) {
      errors.name = t("validation.upstream.nameRequired");
    } else if (upstreamNames.has(name)) {
      errors.name = t("validation.upstream.nameDuplicate");
    } else {
      upstreamNames.set(name, index);
    }
    if (!validUpstreamEndpoint(upstream)) {
      errors.endpoint = t("validation.upstream.endpoint");
    }
    if (upstream.proxy && !config.resolver.proxies.some((proxy) => proxy.name === upstream.proxy)) {
      errors.proxy = t("validation.upstream.proxyMissing");
    }
    if (Object.keys(errors).length) {
      result.resolver.upstreams[index] = errors;
    }
  });

  const proxyNames = new Set<string>();
  config.resolver.proxies.forEach((proxy, index) => {
    const errors: ConfigValidation["resolver"]["proxies"][number] = {};
    const name = proxy.name.trim();
    if (!name) {
      errors.name = t("validation.proxy.nameRequired");
    } else if (proxyNames.has(name)) {
      errors.name = t("validation.proxy.nameDuplicate");
    } else {
      proxyNames.add(name);
    }
    if (!validProxyAddress(proxy)) {
      errors.address = t("validation.proxy.address");
    }
    if (Object.keys(errors).length) {
      result.resolver.proxies[index] = errors;
    }
  });

  config.resolver.hosts.forEach((raw, index) => {
    const row = parseHost(raw);
    const errors: ConfigValidation["resolver"]["hosts"][number] = {};
    if (!isDomain(row.domain)) {
      errors.domain = t("validation.hosts.domain");
    }
    const ips = splitList(row.ips);
    if (!ips.length || ips.some((ip) => !isIpAddress(ip))) {
      errors.ips = t("validation.hosts.ips");
    }
    if (Object.keys(errors).length) {
      result.resolver.hosts[index] = errors;
    }
  });

  config.resolver.routes.forEach((raw, index) => {
    const row = parseRoute(raw);
    const errors: ConfigValidation["resolver"]["routes"][number] = {};
    if (!isRouteDomain(row.domain)) {
      errors.domain = t("validation.routes.domain");
    }
    const missing = row.upstreams.filter((name) => !upstreamNames.has(name));
    if (!row.upstreams.length) {
      errors.upstreams = t("validation.routes.upstreamRequired");
    } else if (missing.length) {
      errors.upstreams = t("validation.routes.upstreamMissing", { names: missing.join(", ") });
    }
    if (Object.keys(errors).length) {
      result.resolver.routes[index] = errors;
    }
  });
}

function validateCache(config: DesktopConfig, result: ConfigValidation, t: Translate) {
  for (const key of ["maxEntries", "maxEntrySize", "minTTL", "maxTTL", "negativeTTL"] as const) {
    if (!isNonNegativeInteger(config.cache[key])) {
      result.cache[key] = t("validation.common.nonNegativeInteger");
    }
  }
  if (config.cache.maxTTL > 0 && config.cache.minTTL > config.cache.maxTTL) {
    result.cache.maxTTL = t("validation.cache.maxTtl");
  }
}

function validateHealthcheck(config: DesktopConfig, result: ConfigValidation, t: Translate) {
  if (!isDuration(config.healthcheck.interval)) {
    result.healthcheck.interval = t("validation.health.interval");
  }
  if (!isDuration(config.healthcheck.timeout)) {
    result.healthcheck.timeout = t("validation.health.timeout");
  }
  if (config.healthcheck.domain.trim() && config.healthcheck.domain.trim() !== "." && !isDomain(config.healthcheck.domain)) {
    result.healthcheck.domain = t("validation.health.domain");
  }
  for (const key of ["failureThreshold", "recoveryThreshold"] as const) {
    if (!isNonNegativeInteger(config.healthcheck[key])) {
      result.healthcheck[key] = t("validation.common.nonNegativeInteger");
    }
  }
}

function defaultTranslate(key: string, values?: Record<string, string | number>): string {
  if (key === "validation.routes.upstreamMissing") {
    return `Referenced upstream does not exist: ${values?.names ?? ""}`;
  }
  return key;
}

function validUpstreamEndpoint(upstream: UpstreamConfig): boolean {
  if (!upstream.host.trim() || !isHost(upstream.host)) {
    return false;
  }
  if (upstream.port.trim() && !isValidPort(upstream.port)) {
    return false;
  }
  const isDoh = upstream.protocol === "http" || upstream.protocol === "https";
  if (!isDoh && upstream.path.trim()) {
    return false;
  }
  if (isDoh && upstream.path.trim() && !upstream.path.startsWith("/")) {
    return false;
  }
  return true;
}

function validProxyAddress(proxy: ProxyConfig): boolean {
  return Boolean(proxy.host.trim() && isHost(proxy.host) && (!proxy.port.trim() || isValidPort(proxy.port)));
}

function parseSocketAddress(value: string): boolean {
  const trimmed = value.trim();
  const parsed = parseHostPort(trimmed);
  return Boolean(parsed && isIpAddress(parsed.host) && isValidPort(parsed.port));
}

function parseBootstrapDns(value: string): boolean {
  const trimmed = value.trim();
  if (!trimmed) {
    return false;
  }
  const parsed = parseHostPort(trimmed);
  if (parsed) {
    return isIpAddress(parsed.host) && isValidPort(parsed.port);
  }
  return isIpAddress(trimmed);
}

function parseHostPort(value: string): { host: string; port: string } | null {
  if (value.startsWith("[")) {
    const end = value.indexOf("]");
    if (end <= 1 || value[end + 1] !== ":") {
      return null;
    }
    return { host: value.slice(1, end), port: value.slice(end + 2) };
  }
  const index = value.lastIndexOf(":");
  if (index <= 0 || index === value.length - 1 || value.slice(0, index).includes(":")) {
    return null;
  }
  return { host: value.slice(0, index), port: value.slice(index + 1) };
}

function isDuration(value: string): boolean {
  const trimmed = value.trim();
  return Boolean(trimmed && /^(?:\d+(?:\.\d+)?(?:ms|s|m|h))+$/.test(trimmed));
}

function isValidPort(value: string): boolean {
  if (!/^\d+$/.test(value.trim())) {
    return false;
  }
  const port = Number(value);
  return Number.isInteger(port) && port >= 1 && port <= 65535;
}

function isNonNegativeInteger(value: number): boolean {
  if (!Number.isFinite(value)) {
    return true;
  }
  return Number.isInteger(value) && value >= 0;
}

function isHost(value: string): boolean {
  return isIpAddress(value) || isDomain(value);
}

function isIpAddress(value: string): boolean {
  const trimmed = value.trim();
  return isIpv4(trimmed) || isIpv6(trimmed);
}

function isIpv4(value: string): boolean {
  const parts = value.split(".");
  return parts.length === 4 && parts.every((part) => /^\d+$/.test(part) && Number(part) >= 0 && Number(part) <= 255);
}

function isIpv6(value: string): boolean {
  return value.includes(":") && /^[0-9a-f:.]+$/i.test(value);
}

function isRouteDomain(value: string): boolean {
  const trimmed = value.trim().replace(/^\*\./, "");
  return isDomain(trimmed);
}

function isDomain(value: string): boolean {
  const trimmed = value.trim().replace(/\.$/, "");
  if (trimmed === "localhost") {
    return true;
  }
  if (!trimmed || trimmed.length > 253 || trimmed.includes("..")) {
    return false;
  }
  return trimmed.split(".").every((label) => /^[a-z0-9-]{1,63}$/i.test(label) && !label.startsWith("-") && !label.endsWith("-"));
}

function splitList(raw: string): string[] {
  return raw
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function collect(value: unknown, messages: string[]) {
  if (!value || typeof value !== "object") {
    return;
  }
  Object.values(value).forEach((item) => {
    if (typeof item === "string" && item) {
      messages.push(item);
    }
  });
}
