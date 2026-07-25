import { Button, Empty, Input, Select, Space, Switch, Table, Tag, Tooltip, type TableColumnsType } from "antd";
import { ArrowDownOutlined, ArrowUpOutlined, DeleteOutlined, PlusOutlined, ReloadOutlined } from "@ant-design/icons";
import { useTranslation } from "react-i18next";

import { proxyProtocolOptions, upstreamProtocolOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import { defaultPortForProtocol, defaultPortForProxy } from "../features/config/transforms";
import type { ConfigValidation } from "../features/config/validation";
import { CommitOnBlurInput } from "../shared/CommitOnBlurInput";
import { FieldWithError } from "../shared/FieldWithError";
import { HintTooltip } from "../shared/HintTooltip";
import { LoadingPanel } from "../shared/LoadingPanel";
import { ParsedInput } from "../shared/ParsedInput";
import type { ProxyConfig, UpstreamConfig } from "../shared/types";

type UpstreamEndpointPatch = Pick<UpstreamConfig, "protocol" | "host" | "port" | "path">;
type ProxyAddressPatch = Pick<ProxyConfig, "host" | "port">;

type UpstreamsPageProps = ConfigPageProps & {
  validation: ConfigValidation["resolver"];
  running: boolean;
  checkingUpstreams: Set<string>;
  onCheckHealth: (upstreamName: string) => void;
};

export function UpstreamsPage({ doc, onChange, validation, running, checkingUpstreams, onCheckHealth }: UpstreamsPageProps) {
  const { t } = useTranslation();

  if (!doc) {
    return <LoadingPanel title={t("upstreams.loadingTitle")} text={t("upstreams.loading")} />;
  }

  const currentDoc = doc;
  const cfg = currentDoc.config;

  function updateResolver(patch: Partial<typeof cfg.resolver>) {
    onChange({ path: currentDoc.path, config: { ...cfg, resolver: { ...cfg.resolver, ...patch } } });
  }

  function updateUpstream(index: number, patch: Partial<UpstreamConfig>) {
    const upstreams = cfg.resolver.upstreams.map((item, i) => (i === index ? { ...item, ...patch } : item));
    updateResolver({ upstreams });
  }

  function addUpstream() {
    updateResolver({
      upstreams: [
        ...cfg.resolver.upstreams,
        { name: `upstream-${cfg.resolver.upstreams.length + 1}`, protocol: "udp", host: "", port: "", path: "", serverName: "", proxy: "" }
      ]
    });
  }

  function removeUpstream(index: number) {
    updateResolver({ upstreams: cfg.resolver.upstreams.filter((_, i) => i !== index) });
  }

  function moveUpstream(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= cfg.resolver.upstreams.length) {
      return;
    }
    const upstreams = [...cfg.resolver.upstreams];
    [upstreams[index], upstreams[target]] = [upstreams[target], upstreams[index]];
    updateResolver({ upstreams });
  }

  function updateProxy(index: number, patch: Partial<ProxyConfig>) {
    const proxies = cfg.resolver.proxies.map((item, i) => (i === index ? { ...item, ...patch } : item));
    updateResolver({ proxies });
  }

  function updateProxyEndpoint(index: number, patch: Partial<Pick<ProxyConfig, "protocol" | "host" | "port">>) {
    updateProxy(index, patch);
  }

  function addProxy() {
    updateResolver({
      proxies: [...cfg.resolver.proxies, { name: `proxy-${cfg.resolver.proxies.length + 1}`, protocol: "socks5", host: "", port: "", username: "", password: "" }]
    });
  }

  function removeProxy(index: number) {
    const removed = cfg.resolver.proxies[index]?.name;
    const proxies = cfg.resolver.proxies.filter((_, i) => i !== index);
    const upstreams = cfg.resolver.upstreams.map((item) => (item.proxy === removed ? { ...item, proxy: "" } : item));
    const defaultProxy = cfg.resolver.defaultProxy === removed ? "" : cfg.resolver.defaultProxy;
    updateResolver({ proxies, upstreams, defaultProxy });
  }

  function updateBootstrapDns(values: string[]) {
    updateResolver({ bootstrapDns: values.map((item) => item.trim()).filter(Boolean) });
  }

  const proxyOptions = [{ value: "", label: t("upstreams.direct") }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))];
  const upstreamRows = cfg.resolver.upstreams.map((item, index) => ({ key: `upstream-${index}`, index, item }));
  const proxyRows = cfg.resolver.proxies.map((item, index) => ({ key: `proxy-${index}`, item, index }));
  // Rows whose endpoint the user has not started filling stay visually quiet: suppressing
  // error display for a fresh row avoids instant red after "add". Validation itself is
  // untouched, so auto-save stays blocked until the row is completed or removed.
  const upstreamRowErrors = (record: (typeof upstreamRows)[number]) =>
    record.item.host.trim() || record.item.port.trim() ? validation.upstreams[record.index] : undefined;
  const proxyRowErrors = (record: (typeof proxyRows)[number]) =>
    record.item.host.trim() || record.item.port.trim() ? validation.proxies[record.index] : undefined;
  const upstreamColumns: TableColumnsType<(typeof upstreamRows)[number]> = [
    {
      title: t("upstreams.order"),
      width: 112,
      fixed: "left",
      render: (_value, record) => (
        <div className="upstreamOrderCell">
          <Tag className="upstreamOrderTag">#{record.index + 1}</Tag>
          <Space size={4}>
            <Button
              size="small"
              icon={<ArrowUpOutlined />}
              onClick={() => moveUpstream(record.index, -1)}
              disabled={record.index === 0}
              aria-label={t("upstreams.moveUp", { name: record.item.name || t("upstreams.numberedUpstream", { index: record.index + 1 }) })}
            />
            <Button
              size="small"
              icon={<ArrowDownOutlined />}
              onClick={() => moveUpstream(record.index, 1)}
              disabled={record.index === cfg.resolver.upstreams.length - 1}
              aria-label={t("upstreams.moveDown", { name: record.item.name || t("upstreams.numberedUpstream", { index: record.index + 1 }) })}
            />
          </Space>
        </div>
      )
    },
    {
      title: t("upstreams.upstreamName"),
      width: 160,
      render: (_value, record) => (
        <FieldWithError error={upstreamRowErrors(record)?.name}>
          <Input status={upstreamRowErrors(record)?.name ? "error" : undefined} value={record.item.name} onChange={(event) => updateUpstream(record.index, { name: event.target.value })} placeholder="cloudflare" />
        </FieldWithError>
      )
    },
    {
      title: t("upstreams.endpoint"),
      width: 310,
      render: (_value, record) => (
        <ParsedInput
          className="upstreamEndpointInput"
          value={formatUpstreamEndpoint(record.item)}
          parse={parseUpstreamEndpoint}
          onApply={(patch) => updateUpstream(record.index, patch)}
          invalidText={t("upstreams.endpointInvalid")}
          externalError={upstreamRowErrors(record)?.endpoint}
          placeholder="udp://1.1.1.1:53"
        />
      )
    },
    {
      title: "SNI",
      width: 160,
      render: (_value, record) => {
        const isDoh = record.item.protocol === "http" || record.item.protocol === "https";
        const serverNameEnabled = record.item.protocol === "dot" || isDoh;
        return (
          <Input
            value={serverNameEnabled ? record.item.serverName : ""}
            onChange={(event) => updateUpstream(record.index, { serverName: event.target.value })}
            placeholder={serverNameEnabled ? "cloudflare-dns.com" : "-"}
            disabled={!serverNameEnabled}
          />
        );
      }
    },
    {
      title: t("upstreams.proxy"),
      width: 140,
      render: (_value, record) => (
        <FieldWithError error={upstreamRowErrors(record)?.proxy}>
          <Select className="workbenchInlineSelect" status={upstreamRowErrors(record)?.proxy ? "error" : undefined} value={record.item.proxy} onChange={(value) => updateUpstream(record.index, { proxy: value })} options={proxyOptions} />
        </FieldWithError>
      )
    },
    {
      title: "",
      width: 104,
      fixed: "right",
      align: "right",
      render: (_value, record) => (
        <Space size={6} className="tableActionButtons">
          <Tooltip title={t("upstreams.checkHealth")}>
            <Button
              icon={<ReloadOutlined />}
              loading={checkingUpstreams.has(record.item.name)}
              onClick={() => onCheckHealth(record.item.name)}
              disabled={!running || !record.item.name.trim()}
              aria-label={t("upstreams.checkHealthFor", { name: record.item.name || t("upstreams.numberedUpstream", { index: record.index + 1 }) })}
            />
          </Tooltip>
          <Button danger icon={<DeleteOutlined />} onClick={() => removeUpstream(record.index)} disabled={cfg.resolver.upstreams.length <= 1} aria-label={t("upstreams.deleteUpstream")} />
        </Space>
      )
    }
  ];
  const proxyColumns: TableColumnsType<(typeof proxyRows)[number]> = [
    {
      title: t("upstreams.name"),
      width: 150,
      render: (_value, record) => (
        <FieldWithError error={proxyRowErrors(record)?.name}>
          <Input status={proxyRowErrors(record)?.name ? "error" : undefined} value={record.item.name} onChange={(event) => updateProxy(record.index, { name: event.target.value })} placeholder={t("upstreams.name")} />
        </FieldWithError>
      )
    },
    {
      title: t("upstreams.protocol"),
      width: 130,
      render: (_value, record) => (
        <Select
          className="workbenchInlineSelect"
          value={record.item.protocol}
          onChange={(value) => updateProxyEndpoint(record.index, { protocol: value, port: record.item.port || (record.item.host ? defaultPortForProxy(value) : "") })}
          options={proxyProtocolOptions}
        />
      )
    },
    {
      title: t("upstreams.address"),
      width: 220,
      render: (_value, record) => (
        <ParsedInput
          className="proxyAddressInput"
          value={formatProxyAddress(record.item)}
          parse={(raw) => parseProxyAddress(raw, record.item)}
          onApply={(patch) => updateProxy(record.index, patch)}
          invalidText={t("upstreams.addressInvalid")}
          externalError={proxyRowErrors(record)?.address}
          placeholder="127.0.0.1:1080"
        />
      )
    },
    {
      title: t("upstreams.username"),
      width: 140,
      render: (_value, record) => (
        <Input value={record.item.username} onChange={(event) => updateProxy(record.index, { username: event.target.value })} placeholder={t("upstreams.optional")} />
      )
    },
    {
      title: t("upstreams.password"),
      width: 140,
      render: (_value, record) => (
        <Input.Password value={record.item.password} onChange={(event) => updateProxy(record.index, { password: event.target.value })} placeholder={t("upstreams.optional")} />
      )
    },
    {
      title: "",
      width: 64,
      fixed: "right",
      align: "right",
      render: (_value, record) => (
        <Button danger icon={<DeleteOutlined />} onClick={() => removeProxy(record.index)} aria-label={t("upstreams.deleteProxy")} />
      )
    }
  ];

  return (
    <section className="pageWorkbench">
      <div className="workbenchToolbar">
        <div className="workbenchToolbarMain">
          <span className="workbenchTitle">{t("upstreams.title")}</span>
        </div>
      </div>

      <main className="workbenchMain">
        <div className="resolverOptionsBar" aria-label={t("upstreams.resolverOptions")}>
          <span className="resolverOptionsTitle">{t("upstreams.resolverOptions")}</span>
          <div className="resolverOptionField resolverOptionFieldNarrow">
            <span>{t("upstreams.timeout")}<HintTooltip hint={t("upstreams.timeoutHint")} /></span>
            <CommitOnBlurInput
              size="small"
              status={validation.timeout ? "error" : undefined}
              title={validation.timeout}
              value={cfg.resolver.timeout}
              onCommit={(value) => updateResolver({ timeout: value })}
              placeholder="5s"
            />
          </div>
          <div className="resolverOptionField resolverOptionFieldWide">
            <span>{t("upstreams.bootstrapDns")}<HintTooltip hint={t("upstreams.bootstrapDnsHint")} /></span>
            <Select
              size="small"
              mode="tags"
              value={cfg.resolver.bootstrapDns}
              onChange={updateBootstrapDns}
              status={Object.keys(validation.bootstrapDns).length ? "error" : undefined}
              placeholder="1.1.1.1:53, 8.8.8.8:53"
              open={false}
              suffixIcon={null}
            />
          </div>
          <div className="resolverOptionField resolverOptionFieldSelect">
            <span>{t("upstreams.defaultProxy")}<HintTooltip hint={t("upstreams.defaultProxyHint")} /></span>
            <Select
              size="small"
              value={cfg.resolver.defaultProxy}
              onChange={(value) => updateResolver({ defaultProxy: value })}
              status={validation.defaultProxy ? "error" : undefined}
              title={validation.defaultProxy}
              options={[{ value: "", label: t("upstreams.none") }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))]}
            />
          </div>
          <div className="resolverOptionSwitch">
            <span>IPv6<HintTooltip hint={t("upstreams.ipv6Hint")} /></span>
            <Switch size="small" checked={cfg.resolver.ipv6Enabled} onChange={(checked) => updateResolver({ ipv6Enabled: checked })} />
          </div>
        </div>

        <div className="workbenchPanel">
          <div className="workbenchPanelHeader">
            <div className="workbenchPanelTitleGroup">
              <span className="workbenchPanelTitle">{t("upstreams.upstreamDns")}</span>
              <Tag>{t("upstreams.count", { count: cfg.resolver.upstreams.length })}</Tag>
            </div>
            <Button type="primary" size="small" icon={<PlusOutlined />} onClick={addUpstream}>{t("upstreams.addUpstream")}</Button>
          </div>
          <div className="workbenchPanelBodyFlush">
          <Table
            rowKey="key"
            size="small"
            pagination={false}
            scroll={{ x: 986 }}
            dataSource={upstreamRows}
            columns={upstreamColumns}
          />
          </div>
        </div>

        <div className="workbenchPanel">
          <div className="workbenchPanelHeader">
            <div className="workbenchPanelTitleGroup">
              <span className="workbenchPanelTitle">{t("upstreams.proxy")}</span>
              <Tag>{t("upstreams.count", { count: cfg.resolver.proxies.length })}</Tag>
            </div>
            <Button type="primary" size="small" icon={<PlusOutlined />} onClick={addProxy}>{t("upstreams.addProxy")}</Button>
          </div>
          <div className="workbenchPanelBodyFlush">
          <Table
            rowKey="key"
            size="small"
            pagination={false}
            scroll={{ x: 844 }}
            dataSource={proxyRows}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("upstreams.noProxy")} /> }}
            columns={proxyColumns}
          />
          </div>
        </div>
      </main>
    </section>
  );
}

function formatUpstreamEndpoint(upstream: UpstreamConfig): string {
  if (!upstream.host.trim()) {
    return "";
  }
  const protocol = normalizeUpstreamProtocol(upstream.protocol) ?? "udp";
  const host = formatEndpointHost(upstream.host);
  const defaultPort = defaultPortForProtocol(protocol);
  const port = upstream.port || defaultPort;
  const portPart = port && !shouldHideEndpointPort(protocol, port, defaultPort) ? `:${port}` : "";
  const path = protocol === "http" || protocol === "https" ? formatEndpointPath(upstream.path) : "";
  return `${protocol}://${host}${portPart}${path}`;
}

function parseUpstreamEndpoint(raw: string): UpstreamEndpointPatch | null {
  const value = normalizeEndpointInput(raw);
  if (!value) {
    return null;
  }
  const source = hasEndpointScheme(value) ? value : `udp://${value}`;
  try {
    const parsed = new URL(source);
    const protocol = normalizeUpstreamProtocol(parsed.protocol.replace(/:$/, ""));
    if (!protocol || !parsed.hostname) {
      return null;
    }
    const isDoh = protocol === "http" || protocol === "https";
    const path = isDoh && parsed.pathname !== "/" ? parsed.pathname : "";
    return {
      protocol,
      host: parsed.hostname.replace(/^\[(.*)\]$/, "$1"),
      port: parsed.port || defaultPortForProtocol(protocol),
      path
    };
  } catch {
    return null;
  }
}

function formatProxyAddress(proxy: ProxyConfig): string {
  if (!proxy.host.trim()) {
    return "";
  }
  const host = formatEndpointHost(proxy.host);
  const port = proxy.port || defaultPortForProxy(proxy.protocol);
  return `${host}${port ? `:${port}` : ""}`;
}

function parseProxyAddress(raw: string, proxy: ProxyConfig | undefined): ProxyAddressPatch | null {
  const value = normalizeEndpointInput(raw);
  if (!value) {
    return null;
  }
  const source = hasEndpointScheme(value) ? value : `socks5://${value}`;
  try {
    const parsed = new URL(source);
    if (!parsed.hostname || (parsed.pathname && parsed.pathname !== "/")) {
      return null;
    }
    return {
      host: parsed.hostname.replace(/^\[(.*)\]$/, "$1"),
      port: parsed.port || defaultPortForProxy(proxy?.protocol ?? "socks5")
    };
  } catch {
    return null;
  }
}

function normalizeUpstreamProtocol(protocol: string): string | null {
  const value = protocol.trim().toLowerCase();
  if (value === "doh") {
    return "https";
  }
  if (value === "quic") {
    return "doq";
  }
  if (upstreamProtocolOptions.some((option) => option.value === value)) {
    return value;
  }
  return null;
}

function normalizeEndpointInput(raw: string): string {
  const value = raw.trim().replace(/[。．]\s*$/, "");
  return value.replace(/(:\d+)\.$/, "$1");
}

function hasEndpointScheme(value: string): boolean {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(value);
}

function formatEndpointHost(host: string): string {
  const value = host.trim();
  return value.includes(":") && !value.startsWith("[") ? `[${value}]` : value;
}

function formatEndpointPath(path: string): string {
  const value = path.trim();
  if (!value) {
    return "";
  }
  return value.startsWith("/") ? value : `/${value}`;
}

function shouldHideEndpointPort(protocol: string, port: string, defaultPort: string): boolean {
  return (protocol === "http" || protocol === "https") && port === defaultPort;
}
