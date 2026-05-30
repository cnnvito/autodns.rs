type SelectOption = {
  value: string;
  label: string;
};

type Translate = (key: string) => string;

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

export function getMatchOptions(t: Translate): SelectOption[] {
  return [
    { value: "exact", label: t("options.match.exact") },
    { value: "suffix", label: t("options.match.suffix") },
    { value: "wildcard", label: t("options.match.wildcard") }
  ];
}

export function getLogLevelOptions(t: Translate): SelectOption[] {
  return [
    { value: "debug", label: t("options.logLevel.debug") },
    { value: "info", label: t("options.logLevel.info") },
    { value: "warn", label: t("options.logLevel.warn") },
    { value: "error", label: t("options.logLevel.error") }
  ];
}
