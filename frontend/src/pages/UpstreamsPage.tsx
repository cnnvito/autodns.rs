import { Button, Card, Empty, Input, Select, Space, Switch, Table, Tag, Typography } from "antd";
import { ArrowDownOutlined, ArrowUpOutlined, DeleteOutlined, PlusOutlined } from "@ant-design/icons";
import { useState } from "react";

import { proxyProtocolOptions, upstreamProtocolOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import { defaultPortForProtocol, defaultPortForProxy } from "../features/config/transforms";
import type { ConfigValidation } from "../features/config/validation";
import type { ProxyConfig, UpstreamConfig } from "../shared/types";

type UpstreamEndpointPatch = Pick<UpstreamConfig, "protocol" | "host" | "port" | "path">;
type ProxyAddressPatch = Pick<ProxyConfig, "host" | "port">;

type UpstreamsPageProps = ConfigPageProps & {
  validation: ConfigValidation["resolver"];
};

export function UpstreamsPage({ doc, onChange, validation }: UpstreamsPageProps) {
  const [endpointDrafts, setEndpointDrafts] = useState<Record<number, string>>({});
  const [proxyAddressDrafts, setProxyAddressDrafts] = useState<Record<number, string>>({});

  if (!doc) {
    return <LoadingPanel />;
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
    setEndpointDrafts({});
  }

  function removeUpstream(index: number) {
    updateResolver({ upstreams: cfg.resolver.upstreams.filter((_, i) => i !== index) });
    setEndpointDrafts({});
  }

  function moveUpstream(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= cfg.resolver.upstreams.length) {
      return;
    }
    const upstreams = [...cfg.resolver.upstreams];
    [upstreams[index], upstreams[target]] = [upstreams[target], upstreams[index]];
    updateResolver({ upstreams });
    setEndpointDrafts({});
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
    setProxyAddressDrafts({});
  }

  function removeProxy(index: number) {
    const removed = cfg.resolver.proxies[index]?.name;
    const proxies = cfg.resolver.proxies.filter((_, i) => i !== index);
    const upstreams = cfg.resolver.upstreams.map((item) => (item.proxy === removed ? { ...item, proxy: "" } : item));
    const defaultProxy = cfg.resolver.defaultProxy === removed ? "" : cfg.resolver.defaultProxy;
    updateResolver({ proxies, upstreams, defaultProxy });
    setProxyAddressDrafts({});
  }

  function updateBootstrapDns(values: string[]) {
    updateResolver({ bootstrapDns: values.map((item) => item.trim()).filter(Boolean) });
  }

  function updateEndpointInput(index: number, value: string) {
    setEndpointDrafts((drafts) => ({ ...drafts, [index]: value }));
    const patch = parseUpstreamEndpoint(value);
    if (patch) {
      updateUpstream(index, patch);
    }
  }

  function commitEndpointInput(index: number) {
    const draft = endpointDrafts[index];
    if (draft === undefined) {
      return;
    }
    const patch = parseUpstreamEndpoint(draft);
    if (patch) {
      updateUpstream(index, patch);
    }
    setEndpointDrafts((drafts) => {
      const next = { ...drafts };
      delete next[index];
      return next;
    });
  }

  function updateProxyAddressInput(index: number, value: string) {
    setProxyAddressDrafts((drafts) => ({ ...drafts, [index]: value }));
    const patch = parseProxyAddress(value, cfg.resolver.proxies[index]);
    if (patch) {
      updateProxy(index, patch);
    }
  }

  function commitProxyAddressInput(index: number) {
    const draft = proxyAddressDrafts[index];
    if (draft === undefined) {
      return;
    }
    const patch = parseProxyAddress(draft, cfg.resolver.proxies[index]);
    if (patch) {
      updateProxy(index, patch);
    }
    setProxyAddressDrafts((drafts) => {
      const next = { ...drafts };
      delete next[index];
      return next;
    });
  }

  const proxyOptions = [{ value: "", label: "直连" }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))];
  const upstreamRows = cfg.resolver.upstreams.map((item, index) => ({ key: `upstream-${index}`, index, item }));
  const proxyRows = cfg.resolver.proxies.map((item, index) => ({ key: `proxy-${index}`, item, index }));

  return (
    <section className="pageWorkbench">
      <div className="workbenchToolbar">
        <div className="workbenchToolbarMain">
          <span className="workbenchTitle">上游与代理</span>
        </div>
      </div>

      <main className="workbenchMain">
        <div className="resolverOptionsBar" aria-label="解析选项">
          <span className="resolverOptionsTitle">解析选项</span>
          <div className="resolverOptionField resolverOptionFieldNarrow">
            <span>超时</span>
            <Input size="small" value={cfg.resolver.timeout} onChange={(event) => updateResolver({ timeout: event.target.value })} placeholder="5s" />
          </div>
          <div className="resolverOptionField resolverOptionFieldWide">
            <span>Fallback DNS</span>
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
            <span>默认代理</span>
            <Select
              size="small"
              value={cfg.resolver.defaultProxy}
              onChange={(value) => updateResolver({ defaultProxy: value })}
              options={[{ value: "", label: "无" }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))]}
            />
          </div>
          <div className="resolverOptionSwitch">
            <span>IPv6</span>
            <Switch size="small" checked={cfg.resolver.ipv6Enabled} onChange={(checked) => updateResolver({ ipv6Enabled: checked })} />
          </div>
        </div>

        <div className="workbenchPanel">
          <div className="workbenchPanelHeader">
            <div className="workbenchPanelTitleGroup">
              <span className="workbenchPanelTitle">上游 DNS</span>
              <Tag>{cfg.resolver.upstreams.length} 个</Tag>
            </div>
            <Button type="primary" size="small" icon={<PlusOutlined />} onClick={addUpstream}>新增上游</Button>
          </div>
          <div className="workbenchPanelBodyFlush">
          <Table
            rowKey="key"
            size="small"
            pagination={false}
            scroll={{ x: "max-content" }}
            dataSource={upstreamRows}
            columns={[
              {
                title: "移动",
                width: 76,
                render: (_value, record) => (
                  <Space.Compact>
                    <Button
                      size="small"
                      icon={<ArrowUpOutlined />}
                      onClick={() => moveUpstream(record.index, -1)}
                      disabled={record.index === 0}
                      aria-label={`上移 ${record.item.name || `第 ${record.index + 1} 个上游`}`}
                    />
                    <Button
                      size="small"
                      icon={<ArrowDownOutlined />}
                      onClick={() => moveUpstream(record.index, 1)}
                      disabled={record.index === cfg.resolver.upstreams.length - 1}
                      aria-label={`下移 ${record.item.name || `第 ${record.index + 1} 个上游`}`}
                    />
                  </Space.Compact>
                )
              },
              { title: "排序", width: 64, render: (_value, record) => record.index + 1 },
              {
                title: "上游标识",
                width: 160,
                render: (_value, record) => (
                  <FieldWithError error={validation.upstreams[record.index]?.name}>
                    <Input status={validation.upstreams[record.index]?.name ? "error" : undefined} value={record.item.name} onChange={(event) => updateUpstream(record.index, { name: event.target.value })} placeholder="cloudflare" />
                  </FieldWithError>
                )
              },
              {
                title: "端点",
                width: 330,
                render: (_value, record) => {
                  const draft = endpointDrafts[record.index];
                  const endpointValue = draft ?? formatUpstreamEndpoint(record.item);
                  const endpointError = draft !== undefined && !parseUpstreamEndpoint(draft)
                    ? "上游端点无效，请检查协议、主机和端口。"
                    : validation.upstreams[record.index]?.endpoint;
                  return (
                    <FieldWithError error={endpointError}>
                      <Input
                        className="upstreamEndpointInput"
                        value={endpointValue}
                        status={endpointError ? "error" : undefined}
                        onChange={(event) => updateEndpointInput(record.index, event.target.value)}
                        onBlur={() => commitEndpointInput(record.index)}
                        onPressEnter={(event) => event.currentTarget.blur()}
                        placeholder="udp://1.1.1.1:53"
                      />
                    </FieldWithError>
                  );
                }
              },
              {
                title: "SNI",
                width: 180,
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
                title: "代理",
                width: 140,
                render: (_value, record) => (
                  <FieldWithError error={validation.upstreams[record.index]?.proxy}>
                    <Select className="workbenchInlineSelect" status={validation.upstreams[record.index]?.proxy ? "error" : undefined} value={record.item.proxy} onChange={(value) => updateUpstream(record.index, { proxy: value })} options={proxyOptions} />
                  </FieldWithError>
                )
              },
              {
                title: "",
                width: 52,
                align: "right",
                render: (_value, record) => (
                  <Button icon={<DeleteOutlined />} onClick={() => removeUpstream(record.index)} disabled={cfg.resolver.upstreams.length <= 1} aria-label="删除上游" />
                )
              }
            ]}
          />
          </div>
        </div>

        <div className="workbenchPanel">
          <div className="workbenchPanelHeader">
            <div className="workbenchPanelTitleGroup">
              <span className="workbenchPanelTitle">代理</span>
              <Tag>{cfg.resolver.proxies.length} 个</Tag>
            </div>
            <Button size="small" icon={<PlusOutlined />} onClick={addProxy}>新增代理</Button>
          </div>
          <div className="workbenchPanelBodyFlush">
          <Table
            rowKey="key"
            size="small"
            pagination={false}
            scroll={{ x: "max-content" }}
            dataSource={proxyRows}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="还没有代理配置，上游会直接连接" /> }}
            columns={[
              {
                title: "名称",
                width: 150,
                render: (_value, record) => (
                  <FieldWithError error={validation.proxies[record.index]?.name}>
                    <Input status={validation.proxies[record.index]?.name ? "error" : undefined} value={record.item.name} onChange={(event) => updateProxy(record.index, { name: event.target.value })} placeholder="名称" />
                  </FieldWithError>
                )
              },
              {
                title: "协议",
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
                title: "地址",
                width: 220,
                render: (_value, record) => {
                  const draft = proxyAddressDrafts[record.index];
                  const addressValue = draft ?? formatProxyAddress(record.item);
                  const addressError = draft !== undefined && !parseProxyAddress(draft, record.item)
                    ? "代理地址无效，请填写主机和 1-65535 端口。"
                    : validation.proxies[record.index]?.address;
                  return (
                    <FieldWithError error={addressError}>
                      <Input
                        className="proxyAddressInput"
                        value={addressValue}
                        status={addressError ? "error" : undefined}
                        onChange={(event) => updateProxyAddressInput(record.index, event.target.value)}
                        onBlur={() => commitProxyAddressInput(record.index)}
                        onPressEnter={(event) => event.currentTarget.blur()}
                        placeholder="127.0.0.1:1080"
                      />
                    </FieldWithError>
                  );
                }
              },
              {
                title: "用户名",
                width: 140,
                render: (_value, record) => (
                  <Input value={record.item.username} onChange={(event) => updateProxy(record.index, { username: event.target.value })} placeholder="可选" />
                )
              },
              {
                title: "密码",
                width: 140,
                render: (_value, record) => (
                  <Input.Password value={record.item.password} onChange={(event) => updateProxy(record.index, { password: event.target.value })} placeholder="可选" />
                )
              },
              {
                title: "",
                width: 52,
                align: "right",
                render: (_value, record) => (
                  <Button icon={<DeleteOutlined />} onClick={() => removeProxy(record.index)} aria-label="删除代理" />
                )
              }
            ]}
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

function LoadingPanel() {
  return (
    <Card title="上游">
      <Typography.Text type="secondary">正在加载本地配置。</Typography.Text>
    </Card>
  );
}

function FieldWithError({ error, children }: { error?: string; children: React.ReactNode }) {
  return (
    <Space direction="vertical" size={4} className="pageFill">
      {children}
      {error ? <Typography.Text type="danger">{error}</Typography.Text> : null}
    </Space>
  );
}
