export type HostRow = {
  domain: string;
  ips: string;
};

export type RouteRow = {
  match: string;
  domain: string;
  upstreams: string[];
};

export type UpstreamEndpointRow = {
  protocol: string;
  host: string;
  port: string;
  path: string;
};

export type ProxyEndpointRow = {
  protocol: string;
  host: string;
  port: string;
};

export function parseHost(raw: string): HostRow {
  const [domain, ips = ""] = raw.split("=");
  return {
    domain: domain.trim(),
    ips: ips.trim()
  };
}

export function formatHost(row: HostRow): string {
  return `${row.domain.trim()}=${row.ips.trim()}`;
}

export function parseRoute(raw: string): RouteRow {
  const [match = "suffix", rest = ""] = raw.split(":");
  const [domain = "", upstreams = ""] = rest.split("=");
  const normalizedMatch = match === "exact" || match === "suffix" || match === "wildcard" ? match : "suffix";
  const domainValue = normalizedMatch === "wildcard" && domain && !domain.startsWith("*.") ? `*.${domain}` : domain;
  return {
    match: normalizedMatch,
    domain: domainValue.trim(),
    upstreams: upstreams
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean)
  };
}

export function formatRoute(row: RouteRow): string {
  const domain = row.match === "wildcard" ? ensureWildcardDomain(row.domain) : stripWildcardPrefix(row.domain);
  return `${row.match}:${domain}=${row.upstreams.join(",")}`;
}

export function defaultRoute(upstream: string): string {
  return formatRoute({ match: "suffix", domain: "example.com", upstreams: upstream ? [upstream] : [] });
}

export function parseUpstreamEndpoint(raw: string): UpstreamEndpointRow {
  const fallback = parseLooseEndpoint(raw, "udp", true);
  try {
    const url = new URL(raw);
    const protocol = url.protocol.replace(":", "") || "udp";
    return {
      protocol,
      host: url.hostname,
      port: url.port,
      path: protocol === "http" || protocol === "https" ? normalizeDohPath(url.pathname) : ""
    };
  } catch {
    return fallback;
  }
}

export function formatUpstreamEndpoint(row: UpstreamEndpointRow): string {
  const protocol = row.protocol || "udp";
  const host = row.host.trim();
  const port = row.port.trim();
  const address = port ? `${host}:${port}` : host;
  if (protocol === "http" || protocol === "https") {
    return `${protocol}://${address}${normalizeDohPath(row.path)}`;
  }
  return `${protocol}://${address}`;
}

export function defaultPortForProtocol(protocol: string): string {
  switch (protocol) {
    case "udp":
    case "tcp":
      return "53";
    case "dot":
    case "doq":
    case "quic":
      return "853";
    case "http":
      return "80";
    case "https":
      return "443";
    default:
      return "";
  }
}

export function parseProxyEndpoint(raw: string): ProxyEndpointRow {
  const fallback = parseLooseEndpoint(raw, "socks5", false);
  try {
    const url = new URL(raw);
    return {
      protocol: url.protocol.replace(":", "") || "socks5",
      host: url.hostname,
      port: url.port
    };
  } catch {
    return fallback;
  }
}

export function formatProxyEndpoint(row: ProxyEndpointRow): string {
  const protocol = row.protocol || "socks5";
  const host = row.host.trim();
  const port = row.port.trim();
  return `${protocol}://${port ? `${host}:${port}` : host}`;
}

export function defaultPortForProxy(protocol: string): string {
  switch (protocol) {
    case "socks5":
      return "1080";
    default:
      return "";
  }
}

function normalizeDohPath(path: string): string {
  const value = path.trim();
  if (!value || value === "/") {
    return "/dns-query";
  }
  return value.startsWith("/") ? value : `/${value}`;
}

function parseLooseEndpoint(raw: string, defaultProtocol: string, includePath: true): UpstreamEndpointRow;
function parseLooseEndpoint(raw: string, defaultProtocol: string, includePath: false): ProxyEndpointRow;
function parseLooseEndpoint(raw: string, defaultProtocol: string, includePath: boolean): UpstreamEndpointRow | ProxyEndpointRow {
  const trimmed = raw.trim();
  const match = trimmed.match(/^([a-z][a-z0-9+.-]*):\/\/(.*)$/i);
  const protocol = match?.[1] || defaultProtocol;
  let rest = match?.[2] ?? trimmed;
  let path = "";
  if (includePath) {
    const slashIndex = rest.indexOf("/");
    if (slashIndex >= 0) {
      path = rest.slice(slashIndex);
      rest = rest.slice(0, slashIndex);
    }
  }

  const { host, port } = splitLooseHostPort(rest);
  if (includePath) {
    return { protocol, host, port, path };
  }
  return { protocol, host, port };
}

function splitLooseHostPort(value: string): { host: string; port: string } {
  if (!value) {
    return { host: "", port: "" };
  }
  if (value.startsWith("[")) {
    const end = value.indexOf("]");
    if (end >= 0) {
      const tail = value.slice(end + 1);
      return {
        host: value.slice(1, end),
        port: tail.startsWith(":") ? tail.slice(1) : ""
      };
    }
  }
  const colonIndex = value.lastIndexOf(":");
  if (colonIndex > -1 && value.indexOf(":") === colonIndex) {
    return {
      host: value.slice(0, colonIndex),
      port: value.slice(colonIndex + 1)
    };
  }
  return { host: value, port: "" };
}

function ensureWildcardDomain(domain: string): string {
  const value = domain.trim();
  if (!value) {
    return "";
  }
  return value.startsWith("*.") ? value : `*.${value}`;
}

function stripWildcardPrefix(domain: string): string {
  return domain.trim().replace(/^\*\./, "");
}
