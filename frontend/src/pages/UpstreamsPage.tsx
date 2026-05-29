import { Button, Card, Col, Empty, Form, Input, Row, Select, Space, Switch, Table, Tag, Typography } from "antd";
import { DeleteOutlined, HolderOutlined, PlusOutlined } from "@ant-design/icons";
import type { DragEvent, KeyboardEvent } from "react";
import { useEffect, useMemo, useState } from "react";

import { proxyProtocolOptions, upstreamProtocolOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import { defaultPortForProtocol, defaultPortForProxy } from "../features/config/transforms";
import type { ProxyConfig, UpstreamConfig } from "../shared/types";

type DragState = {
  fromIndex: number | null;
  overIndex: number | null;
  order: number[];
};

export function UpstreamsPage({ doc, onChange }: ConfigPageProps) {
  const upstreams = doc?.config.resolver.upstreams ?? [];
  const naturalOrder = useMemo(() => upstreams.map((_, index) => index), [upstreams]);
  const [dragState, setDragState] = useState<DragState>({ fromIndex: null, overIndex: null, order: [] });

  useEffect(() => {
    setDragState((current) => {
      if (current.fromIndex !== null) {
        return current;
      }
      return { fromIndex: null, overIndex: null, order: naturalOrder };
    });
  }, [naturalOrder]);

  if (!doc) {
    return <LoadingPanel />;
  }

  const currentDoc = doc;
  const cfg = currentDoc.config;
  const isDragging = dragState.fromIndex !== null;
  const visibleOrder = isDragging && dragState.order.length === cfg.resolver.upstreams.length ? dragState.order : naturalOrder;

  function updateResolver(patch: Partial<typeof cfg.resolver>) {
    onChange({ path: currentDoc.path, config: { ...cfg, resolver: { ...cfg.resolver, ...patch } } });
  }

  function updateUpstream(index: number, patch: Partial<UpstreamConfig>) {
    const upstreams = cfg.resolver.upstreams.map((item, i) => (i === index ? { ...item, ...patch } : item));
    updateResolver({ upstreams });
  }

  function updateEndpoint(index: number, patch: Partial<Pick<UpstreamConfig, "protocol" | "host" | "port" | "path">>) {
    updateUpstream(index, patch);
  }

  function addUpstream() {
    updateResolver({
      upstreams: [
        ...cfg.resolver.upstreams,
        { name: `upstream-${cfg.resolver.upstreams.length + 1}`, protocol: "udp", host: "1.1.1.1", port: "", path: "", serverName: "", proxy: "" }
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

  function startDrag(index: number, event: DragEvent<HTMLElement>) {
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", String(index));
    setDragState({ fromIndex: index, overIndex: index, order: naturalOrder });
  }

  function enterDropTarget(targetIndex: number) {
    setDragState((current) => {
      if (current.fromIndex === null || current.overIndex === targetIndex) {
        return current;
      }
      const order = current.order.length === cfg.resolver.upstreams.length ? current.order : naturalOrder;
      const fromPosition = order.indexOf(current.fromIndex);
      const targetPosition = order.indexOf(targetIndex);
      if (fromPosition < 0 || targetPosition < 0) {
        return current;
      }
      return {
        ...current,
        overIndex: targetIndex,
        order: moveOrderItem(order, fromPosition, targetPosition)
      };
    });
  }

  function allowDrop(event: DragEvent<HTMLDivElement>) {
    if (dragState.fromIndex === null) {
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
  }

  function commitDrag(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();
    const order = dragState.order.length === cfg.resolver.upstreams.length ? dragState.order : naturalOrder;
    const changed = order.some((item, index) => item !== index);
    if (changed) {
      updateResolver({ upstreams: order.map((index) => cfg.resolver.upstreams[index]) });
    }
    resetDrag();
  }

  function resetDrag() {
    setDragState({ fromIndex: null, overIndex: null, order: naturalOrder });
  }

  function handleDragHandleKeyDown(index: number, event: KeyboardEvent<HTMLElement>) {
    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveUpstream(index, -1);
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveUpstream(index, 1);
    }
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
      proxies: [...cfg.resolver.proxies, { name: `proxy-${cfg.resolver.proxies.length + 1}`, protocol: "socks5", host: "127.0.0.1", port: "1080", username: "", password: "" }]
    });
  }

  function removeProxy(index: number) {
    const removed = cfg.resolver.proxies[index]?.name;
    const proxies = cfg.resolver.proxies.filter((_, i) => i !== index);
    const upstreams = cfg.resolver.upstreams.map((item) => (item.proxy === removed ? { ...item, proxy: "" } : item));
    const defaultProxy = cfg.resolver.defaultProxy === removed ? "" : cfg.resolver.defaultProxy;
    updateResolver({ proxies, upstreams, defaultProxy });
  }

  function updateBootstrapDns(value: string) {
    updateResolver({ bootstrapDns: splitBootstrapDns(value) });
  }

  const proxyOptions = [{ value: "", label: "直连" }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))];
  const bootstrapDns = cfg.resolver.bootstrapDns.join(", ");
  const upstreamRows = visibleOrder.map((originalIndex, visualIndex) => ({ key: `upstream-${originalIndex}`, originalIndex, visualIndex, item: cfg.resolver.upstreams[originalIndex] }));
  const proxyRows = cfg.resolver.proxies.map((item, index) => ({ key: `proxy-${index}`, item, index }));

  return (
    <section className="pageWorkbench">
      <div className="workbenchToolbar">
        <div className="workbenchToolbarMain">
          <span className="workbenchTitle">上游与代理</span>
          <Tag>{cfg.resolver.upstreams.length} 个上游</Tag>
          <Tag>{cfg.resolver.proxies.length} 个代理</Tag>
          <Typography.Text type="secondary">按列表顺序解析，拿到答案即返回。</Typography.Text>
        </div>
        <div className="workbenchToolbarActions">
          <Button icon={<PlusOutlined />} onClick={addProxy}>新增代理</Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={addUpstream}>新增上游</Button>
        </div>
      </div>

      <main className="workbenchMain">
        <div className="workbenchPanel">
          <div className="workbenchPanelHeader">
            <span className="workbenchPanelTitle">解析选项</span>
            <Typography.Text type="secondary">这些设置会影响全部上游请求。</Typography.Text>
          </div>
          <div className="workbenchPanelBody">
            <Row gutter={[12, 8]}>
              <Col xs={24} md={8}>
                <Form.Item label="解析超时" layout="vertical">
                  <Input value={cfg.resolver.timeout} onChange={(event) => updateResolver({ timeout: event.target.value })} placeholder="5s" />
                </Form.Item>
              </Col>
              <Col xs={24} md={10}>
                <Form.Item label="上游域名 fallback" layout="vertical">
                  <Input value={bootstrapDns} onChange={(event) => updateBootstrapDns(event.target.value)} placeholder="1.1.1.1:53, 8.8.8.8:53" />
                </Form.Item>
              </Col>
              <Col xs={24} md={6}>
                <Form.Item label="IPv6" layout="vertical">
                  <Switch checkedChildren="启用" unCheckedChildren="关闭" checked={cfg.resolver.ipv6Enabled} onChange={(checked) => updateResolver({ ipv6Enabled: checked })} />
                </Form.Item>
              </Col>
              <Col xs={24} md={8}>
                <Form.Item label="默认代理" layout="vertical">
                  <Select
                    value={cfg.resolver.defaultProxy}
                    onChange={(value) => updateResolver({ defaultProxy: value })}
                    options={[{ value: "", label: "无" }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))]}
                  />
                </Form.Item>
              </Col>
            </Row>
          </div>
        </div>

        <div className="workbenchPanel">
          <div className="workbenchPanelHeader">
            <span className="workbenchPanelTitle">上游 DNS</span>
            <Typography.Text type="secondary">拖动左侧把手调整优先级；无答案或出错时继续下一个上游。</Typography.Text>
          </div>
          <div className="workbenchPanelBodyFlush">
          <Table
            rowKey="key"
            size="small"
            pagination={false}
            scroll={{ x: "max-content" }}
            dataSource={upstreamRows}
            onRow={(record) => ({
              draggable: true,
              onDragStart: (event) => startDrag(record.originalIndex, event),
              onDragEnter: () => enterDropTarget(record.originalIndex),
              onDragOver: allowDrop,
              onDrop: commitDrag,
              onDragEnd: resetDrag,
              style: {
                opacity: dragState.fromIndex === record.originalIndex ? 0.58 : 1,
                transform: dragState.overIndex === record.originalIndex && isDragging ? "translateY(-1px)" : undefined
              }
            })}
            columns={[
              {
                title: "",
                width: 44,
                render: (_value, record) => (
                  <Button
                    icon={<HolderOutlined />}
                    onKeyDown={(event) => handleDragHandleKeyDown(record.originalIndex, event)}
                    aria-label={`拖动 ${record.item.name || `第 ${record.visualIndex + 1} 个上游`} 调整顺序`}
                    title="拖动排序，键盘可用上下方向键"
                  />
                )
              },
              { title: "排序", width: 64, render: (_value, record) => record.visualIndex + 1 },
              {
                title: "上游标识",
                width: 160,
                render: (_value, record) => (
                  <Input value={record.item.name} onChange={(event) => updateUpstream(record.originalIndex, { name: event.target.value })} placeholder="cloudflare" />
                )
              },
              {
                title: "协议",
                width: 130,
                render: (_value, record) => (
                  <Select
                    className="workbenchInlineSelect"
                    value={record.item.protocol}
                    onChange={(value) => updateEndpoint(record.originalIndex, { protocol: value, port: record.item.port || defaultPortForProtocol(value) })}
                    options={upstreamProtocolOptions}
                  />
                )
              },
              {
                title: "主机",
                width: 180,
                render: (_value, record) => (
                  <Input value={record.item.host} onChange={(event) => updateEndpoint(record.originalIndex, { host: event.target.value })} placeholder="1.1.1.1" />
                )
              },
              {
                title: "端口",
                width: 100,
                render: (_value, record) => (
                  <Input value={record.item.port} onChange={(event) => updateEndpoint(record.originalIndex, { port: event.target.value })} placeholder={defaultPortForProtocol(record.item.protocol)} />
                )
              },
              {
                title: "路径",
                width: 150,
                render: (_value, record) => {
                  const isDoh = record.item.protocol === "http" || record.item.protocol === "https";
                  return (
                    <Input
                      value={isDoh ? record.item.path : ""}
                      onChange={(event) => updateEndpoint(record.originalIndex, { path: event.target.value })}
                      placeholder={isDoh ? "/dns-query" : "-"}
                      disabled={!isDoh}
                    />
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
                      onChange={(event) => updateUpstream(record.originalIndex, { serverName: event.target.value })}
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
                  <Select className="workbenchInlineSelect" value={record.item.proxy} onChange={(value) => updateUpstream(record.originalIndex, { proxy: value })} options={proxyOptions} />
                )
              },
              {
                title: "",
                width: 52,
                align: "right",
                render: (_value, record) => (
                  <Button icon={<DeleteOutlined />} onClick={() => removeUpstream(record.originalIndex)} disabled={cfg.resolver.upstreams.length <= 1} aria-label="删除上游" />
                )
              }
            ]}
          />
          </div>
        </div>

        <div className="workbenchPanel">
          <div className="workbenchPanelHeader">
            <span className="workbenchPanelTitle">代理</span>
            <Typography.Text type="secondary">默认代理只作用于未单独指定代理的上游。</Typography.Text>
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
                  <Input value={record.item.name} onChange={(event) => updateProxy(record.index, { name: event.target.value })} placeholder="名称" />
                )
              },
              {
                title: "协议",
                width: 130,
                render: (_value, record) => (
                  <Select
                    className="workbenchInlineSelect"
                    value={record.item.protocol}
                    onChange={(value) => updateProxyEndpoint(record.index, { protocol: value, port: record.item.port || defaultPortForProxy(value) })}
                    options={proxyProtocolOptions}
                  />
                )
              },
              {
                title: "主机",
                width: 180,
                render: (_value, record) => (
                  <Input value={record.item.host} onChange={(event) => updateProxyEndpoint(record.index, { host: event.target.value })} placeholder="127.0.0.1" />
                )
              },
              {
                title: "端口",
                width: 100,
                render: (_value, record) => (
                  <Input value={record.item.port} onChange={(event) => updateProxyEndpoint(record.index, { port: event.target.value })} placeholder={defaultPortForProxy(record.item.protocol)} />
                )
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

function moveOrderItem(order: number[], fromPosition: number, targetPosition: number): number[] {
  const next = [...order];
  const [item] = next.splice(fromPosition, 1);
  next.splice(targetPosition, 0, item);
  return next;
}

function splitBootstrapDns(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function LoadingPanel() {
  return (
    <Card title="上游">
      <Typography.Text type="secondary">正在加载本地配置。</Typography.Text>
    </Card>
  );
}
