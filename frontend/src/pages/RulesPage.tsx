import { Button, Card, Empty, Input, List, Modal, Segmented, Select, Space, Table, Tag, Typography } from "antd";
import { CloseOutlined, DeleteOutlined, ImportOutlined, PlusOutlined } from "@ant-design/icons";
import { useState } from "react";

import { matchOptions } from "../features/config/options";
import { defaultRoute, formatHost, formatRoute, parseHost, parseRoute } from "../features/config/transforms";
import type { ConfigPageProps } from "../features/config/doc";

type ImportKind = "hosts" | "routes";

type ImportPreviewItem = {
  raw: string;
  value: string;
  summary: string;
  valid: boolean;
  reason: string;
};

export function RulesPage({ doc, onChange }: ConfigPageProps) {
  const [importKind, setImportKind] = useState<ImportKind | null>(null);
  const [importDraft, setImportDraft] = useState("");
  const [activeRuleKind, setActiveRuleKind] = useState<ImportKind>("hosts");

  if (!doc) {
    return <LoadingPanel />;
  }

  const currentDoc = doc;
  const cfg = currentDoc.config;

  function updateResolver(patch: Partial<typeof cfg.resolver>) {
    onChange({ path: currentDoc.path, config: { ...cfg, resolver: { ...cfg.resolver, ...patch } } });
  }

  function updateHost(index: number, row: ReturnType<typeof parseHost>) {
    const hosts = cfg.resolver.hosts.map((item, i) => (i === index ? formatHost(row) : item));
    updateResolver({ hosts });
  }

  function removeHost(index: number) {
    updateResolver({ hosts: cfg.resolver.hosts.filter((_, i) => i !== index) });
  }

  function updateRoute(index: number, row: ReturnType<typeof parseRoute>) {
    const routes = cfg.resolver.routes.map((item, i) => (i === index ? formatRoute(row) : item));
    updateResolver({ routes });
  }

  function removeRoute(index: number) {
    updateResolver({ routes: cfg.resolver.routes.filter((_, i) => i !== index) });
  }

  function appendRouteUpstream(index: number, name: string) {
    if (!name) {
      return;
    }
    const row = parseRoute(cfg.resolver.routes[index]);
    if (row.upstreams.includes(name)) {
      return;
    }
    updateRoute(index, { ...row, upstreams: [...row.upstreams, name] });
  }

  function removeRouteUpstream(index: number, name: string) {
    const row = parseRoute(cfg.resolver.routes[index]);
    updateRoute(index, { ...row, upstreams: row.upstreams.filter((item) => item !== name) });
  }

  function routeUpstreamOptions(selected: string[]) {
    const available = cfg.resolver.upstreams
      .filter((item) => !selected.includes(item.name))
      .map((item) => ({ value: item.name, label: item.name }));
    if (available.length === 0) {
      return [{ value: "", label: selected.length ? "已选择全部上游" : "暂无上游" }];
    }
    return [{ value: "", label: "添加上游" }, ...available];
  }

  const importPreview = importKind === "hosts"
    ? parseHostImport(importDraft, cfg.resolver.hosts)
    : importKind === "routes"
      ? parseRouteImport(importDraft, cfg.resolver.routes, cfg.resolver.upstreams[0]?.name || "")
      : [];
  const importableItems = importPreview.filter((item) => item.valid);
  const hostRows = cfg.resolver.hosts.map((raw, index) => ({ key: `host-${index}`, index, row: parseHost(raw) }));
  const routeRows = cfg.resolver.routes.map((raw, index) => ({ key: `route-${index}`, index, row: parseRoute(raw) }));

  function openImport(kind: ImportKind) {
    setImportKind(kind);
    setImportDraft(kind === "hosts" ? "127.0.0.1 example.local\n::1 ipv6.local" : `suffix:example.com=${cfg.resolver.upstreams[0]?.name || "upstream-1"}`);
  }

  function closeImport() {
    setImportKind(null);
    setImportDraft("");
  }

  function commitImport() {
    if (!importKind || importableItems.length === 0) {
      return;
    }
    if (importKind === "hosts") {
      updateResolver({ hosts: [...cfg.resolver.hosts, ...importableItems.map((item) => item.value)] });
    } else {
      updateResolver({ routes: [...cfg.resolver.routes, ...importableItems.map((item) => item.value)] });
    }
    closeImport();
  }

  return (
    <>
      <section className="pageWorkbench">
        <div className="workbenchToolbar">
          <div className="workbenchToolbarMain">
            <span className="workbenchTitle">规则工作台</span>
            <Segmented
              value={activeRuleKind}
              onChange={(value) => setActiveRuleKind(value as ImportKind)}
              options={[
                { value: "hosts", label: "固定解析" },
                { value: "routes", label: "路由规则" }
              ]}
            />
            <Tag>{cfg.resolver.hosts.length} 条固定解析</Tag>
            <Tag>{cfg.resolver.routes.length} 条路由</Tag>
          </div>
          <div className="workbenchToolbarActions">
            <Button icon={<ImportOutlined />} onClick={() => openImport(activeRuleKind)}>批量导入</Button>
            {activeRuleKind === "hosts" ? (
              <Button type="primary" icon={<PlusOutlined />} onClick={() => updateResolver({ hosts: [...cfg.resolver.hosts, "example.local=127.0.0.1"] })}>新增解析</Button>
            ) : (
              <Button type="primary" icon={<PlusOutlined />} onClick={() => updateResolver({ routes: [...cfg.resolver.routes, defaultRoute(cfg.resolver.upstreams[0]?.name || "")] })}>新增路由</Button>
            )}
          </div>
        </div>

        <main className="workbenchMain">
          {activeRuleKind === "hosts" ? (
            <div className="workbenchPanel">
              <div className="workbenchPanelHeader">
                <span className="workbenchPanelTitle">固定解析</span>
                <Typography.Text type="secondary">命中后直接返回这里的 IP，不再请求上游。</Typography.Text>
              </div>
              <div className="workbenchPanelBodyFlush">
                <Table
                  rowKey="key"
                  size="small"
                  pagination={false}
                  scroll={{ x: "max-content" }}
                  dataSource={hostRows}
                  locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="还没有固定解析记录" /> }}
                  columns={[
                    {
                      title: "域名",
                      dataIndex: ["row", "domain"],
                      render: (_value, record) => (
                        <Input value={record.row.domain} onChange={(event) => updateHost(record.index, { ...record.row, domain: event.target.value })} placeholder="example.local" />
                      )
                    },
                    {
                      title: "IP 地址",
                      dataIndex: ["row", "ips"],
                      render: (_value, record) => (
                        <Input value={record.row.ips} onChange={(event) => updateHost(record.index, { ...record.row, ips: event.target.value })} placeholder="127.0.0.1, ::1" />
                      )
                    },
                    {
                      title: "",
                      width: 48,
                      align: "right",
                      render: (_value, record) => (
                        <Button icon={<DeleteOutlined />} onClick={() => removeHost(record.index)} aria-label="删除固定解析" />
                      )
                    }
                  ]}
                />
              </div>
            </div>
          ) : (
            <div className="workbenchPanel">
              <div className="workbenchPanelHeader">
                <span className="workbenchPanelTitle">路由规则</span>
                <Typography.Text type="secondary">命中的域名走指定上游；未命中时按上游顺序解析。</Typography.Text>
              </div>
              <div className="workbenchPanelBodyFlush">
                <Table
                  rowKey="key"
                  size="small"
                  pagination={false}
                  scroll={{ x: "max-content" }}
                  dataSource={routeRows}
                  locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="还没有自定义路由，默认会按上游顺序解析" /> }}
                  columns={[
                    {
                      title: "匹配方式",
                      width: 150,
                      render: (_value, record) => (
                        <Select className="workbenchInlineSelect" value={record.row.match} onChange={(value) => updateRoute(record.index, { ...record.row, match: value })} options={matchOptions} />
                      )
                    },
                    {
                      title: "域名",
                      width: 220,
                      render: (_value, record) => (
                        <Input value={record.row.domain} onChange={(event) => updateRoute(record.index, { ...record.row, domain: event.target.value })} placeholder="example.com" />
                      )
                    },
                    {
                      title: "目标上游",
                      render: (_value, record) => (
                        <Space orientation="vertical" size={8} className="pageFill">
                          <Space size={[4, 4]} wrap>
                            {record.row.upstreams.map((name) => {
                              const missingUpstream = !cfg.resolver.upstreams.some((item) => item.name === name);
                              return (
                                <Tag
                                  key={name}
                                  color={missingUpstream ? "error" : "processing"}
                                  closable
                                  closeIcon={<CloseOutlined />}
                                  onClose={() => removeRouteUpstream(record.index, name)}
                                >
                                  {name}{missingUpstream ? "（已失效）" : ""}
                                </Tag>
                              );
                            })}
                            {record.row.upstreams.length === 0 ? <Tag color="warning">未选择上游</Tag> : null}
                          </Space>
                          <Select
                            className="workbenchInlineSelect"
                            value=""
                            onChange={(value) => appendRouteUpstream(record.index, value)}
                            options={routeUpstreamOptions(record.row.upstreams)}
                          />
                        </Space>
                      )
                    },
                    {
                      title: "状态",
                      width: 180,
                      render: (_value, record) => {
                        const status = cfg.resolver.routeStatuses?.[record.index];
                        const missing = record.row.upstreams.filter((name) => !cfg.resolver.upstreams.some((item) => item.name === name));
                        const invalidReason = status?.invalidReason || (!record.row.upstreams.length ? "未选择上游" : missing.length ? `引用的上游已删除：${missing.join(", ")}` : "");
                        return (
                          <Space orientation="vertical" size={4}>
                            <Tag color={invalidReason ? "error" : "success"}>{invalidReason ? "已失效" : "有效"}</Tag>
                            {invalidReason ? <Typography.Text type="danger">{invalidReason}</Typography.Text> : null}
                          </Space>
                        );
                      }
                    },
                    {
                      title: "",
                      width: 48,
                      align: "right",
                      render: (_value, record) => (
                        <Button icon={<DeleteOutlined />} onClick={() => removeRoute(record.index)} aria-label="删除路由" />
                      )
                    }
                  ]}
                />
              </div>
            </div>
          )}
        </main>
      </section>

      <Modal
        open={Boolean(importKind)}
        title={importKind === "hosts" ? "批量导入固定解析" : "批量导入路由规则"}
        width={720}
        okText={`导入 ${importableItems.length} 条`}
        okButtonProps={{ disabled: importableItems.length === 0 }}
        cancelText="取消"
        onOk={commitImport}
        onCancel={closeImport}
      >
        <Space orientation="vertical" size={12} className="pageFill">
          <Typography.Text type="secondary">
            {importKind === "hosts" ? "支持 hosts 文件格式，也支持 domain=ip1,ip2。" : "支持 suffix:domain=upstream1,upstream2；省略匹配方式时默认 suffix。"}
          </Typography.Text>
          <Input.TextArea
            value={importDraft}
            onChange={(event) => setImportDraft(event.target.value)}
            spellCheck={false}
            autoSize={{ minRows: 8, maxRows: 12 }}
          />
          <Space>
            <Tag>{importPreview.length} 行已解析</Tag>
            <Tag color="processing">{importableItems.length} 条可导入</Tag>
          </Space>
          <List
            size="small"
            bordered
            dataSource={importPreview}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="粘贴内容后会在这里预览" /> }}
            renderItem={(item, index) => (
              <List.Item key={`${item.raw}-${index}`} extra={<Tag color={item.valid ? "success" : "error"}>{item.valid ? "可导入" : item.reason}</Tag>}>
                <List.Item.Meta title={item.summary} description={item.raw} />
              </List.Item>
            )}
          />
        </Space>
      </Modal>
    </>
  );
}

function parseHostImport(raw: string, existing: string[]): ImportPreviewItem[] {
  const seen = new Set(existing.map((item) => parseHost(item).domain.toLowerCase()).filter(Boolean));
  return contentLines(raw).flatMap((line) => {
    const parsedRows = parseHostImportLine(line);
    if (parsedRows.length === 0) {
      return [];
    }
    return parsedRows.map((parsed) => {
      const key = parsed.domain.toLowerCase();
      const duplicate = seen.has(key);
      if (!duplicate) {
        seen.add(key);
      }
      const value = formatHost({ domain: parsed.domain, ips: parsed.ips.join(", ") });
      return {
        raw: line,
        value,
        summary: `${parsed.domain} -> ${parsed.ips.join(", ")}`,
        valid: !duplicate,
        reason: duplicate ? "域名已存在，已跳过" : ""
      };
    });
  });
}

function parseHostImportLine(line: string): { domain: string; ips: string[] }[] {
  const equalParts = line.split("=");
  if (equalParts.length === 2) {
    const domain = equalParts[0].trim();
    const ips = splitList(equalParts[1]);
    return domain && ips.length ? [{ domain, ips }] : [];
  }
  const parts = line.split(/\s+/).filter(Boolean);
  if (parts.length < 2) {
    return [];
  }
  const [first, second, ...rest] = parts;
  if (looksLikeIp(first)) {
    return [second, ...rest]
      .filter((domain) => !looksLikeIp(domain))
      .map((domain) => ({ domain, ips: [first] }));
  }
  if (looksLikeIp(second)) {
    return [{ domain: first, ips: [second, ...rest.filter(looksLikeIp)] }];
  }
  return [];
}

function parseRouteImport(raw: string, existing: string[], fallbackUpstream: string): ImportPreviewItem[] {
  const seen = new Set(existing.map(routeKey));
  return contentLines(raw).flatMap((line) => {
    const row = parseRouteImportLine(line, fallbackUpstream);
    if (!row) {
      return [];
    }
    const value = formatRoute(row);
    const key = routeKey(value);
    const duplicate = seen.has(key);
    if (!duplicate) {
      seen.add(key);
    }
    return [{
      raw: line,
      value,
      summary: `${row.match}:${row.domain} -> ${row.upstreams.join(", ") || "未选择上游"}`,
      valid: !duplicate && row.upstreams.length > 0,
      reason: duplicate ? "路由已存在，已跳过" : "未指定上游"
    }];
  });
}

function parseRouteImportLine(line: string, fallbackUpstream: string): ReturnType<typeof parseRoute> | null {
  if (line.includes(":") && line.includes("=")) {
    return parseRoute(line);
  }
  const [domainPart, upstreamPart = ""] = line.split("=");
  const tokens = domainPart.trim().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) {
    return null;
  }
  let match = "suffix";
  let domain = tokens[0];
  if (tokens[0] === "exact" || tokens[0] === "suffix" || tokens[0] === "wildcard") {
    match = tokens[0];
    domain = tokens[1] || "";
  }
  const upstreams = splitList(upstreamPart || tokens.slice(match === tokens[0] ? 2 : 1).join(","));
  return { match, domain, upstreams: upstreams.length ? upstreams : fallbackUpstream ? [fallbackUpstream] : [] };
}

function contentLines(raw: string): string[] {
  return raw
    .split(/\r?\n/)
    .map((line) => line.replace(/\s+#.*$/, "").trim())
    .filter(Boolean);
}

function splitList(raw: string): string[] {
  return raw
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function looksLikeIp(value: string): boolean {
  return /^(\d{1,3}\.){3}\d{1,3}$/.test(value) || /^[0-9a-f:]+$/i.test(value);
}

function routeKey(raw: string): string {
  const row = parseRoute(raw);
  return `${row.match}:${row.domain.toLowerCase()}=${row.upstreams.join(",")}`;
}

function LoadingPanel() {
  return (
    <Card title="规则">
      <Typography.Text type="secondary">正在加载本地配置。</Typography.Text>
    </Card>
  );
}
