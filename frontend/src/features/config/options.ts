import type { SelectOption } from "../../shared/ui";

export const serverModeOptions: SelectOption[] = [
  { value: "udp", label: "UDP" },
  { value: "tcp", label: "TCP" },
  { value: "doh", label: "DoH" },
  { value: "dot", label: "DoT" }
];

export const upstreamProtocolOptions: SelectOption[] = [
  { value: "udp", label: "UDP" },
  { value: "tcp", label: "TCP" },
  { value: "dot", label: "DoT" },
  { value: "doq", label: "DoQ QUIC" },
  { value: "https", label: "DoH HTTPS" },
  { value: "http", label: "DoH HTTP" }
];

export const proxyProtocolOptions: SelectOption[] = [
  { value: "socks5", label: "SOCKS5" }
];

export const matchOptions: SelectOption[] = [
  { value: "exact", label: "精确匹配" },
  { value: "suffix", label: "后缀匹配" },
  { value: "wildcard", label: "通配匹配" }
];

export const logLevelOptions: SelectOption[] = [
  { value: "debug", label: "调试" },
  { value: "info", label: "信息" },
  { value: "warn", label: "警告" },
  { value: "error", label: "错误" }
];
