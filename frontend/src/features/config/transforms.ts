export type HostRow = {
  domain: string;
  ips: string;
};

export type RouteRow = {
  match: string;
  domain: string;
  upstreams: string[];
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
  return formatRoute({ match: "suffix", domain: "", upstreams: upstream ? [upstream] : [] });
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

export function defaultPortForProxy(protocol: string): string {
  switch (protocol) {
    case "socks5":
      return "1080";
    default:
      return "";
  }
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
