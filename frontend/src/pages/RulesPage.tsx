import { Button, Card, Empty, Input, List, Modal, Segmented, Select, Space, Table, Tag, Typography } from "antd";
import { DeleteOutlined, ImportOutlined, PlusOutlined } from "@ant-design/icons";
import type { SelectProps } from "antd";
import type { MouseEvent, ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { getMatchOptions } from "../features/config/options";
import { defaultRoute, formatHost, formatRoute, parseHost, parseRoute } from "../features/config/transforms";
import type { ConfigPageProps } from "../features/config/doc";
import type { ConfigValidation } from "../features/config/validation";

type ImportKind = "hosts" | "routes";

type ImportPreviewItem = {
  raw: string;
  value: string;
  summary: string;
  valid: boolean;
  reason: string;
};

type TagRender = SelectProps["tagRender"];
type Translate = (key: string, values?: Record<string, string | number>) => string;

type RulesPageProps = ConfigPageProps & {
  validation: ConfigValidation["resolver"];
};

export function RulesPage({ doc, onChange, validation }: RulesPageProps) {
  const { t } = useTranslation();
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

  const importPreview = importKind === "hosts"
    ? parseHostImport(importDraft, cfg.resolver.hosts, t)
    : importKind === "routes"
      ? parseRouteImport(importDraft, cfg.resolver.routes, cfg.resolver.upstreams[0]?.name || "", t)
      : [];
  const importableItems = importPreview.filter((item) => item.valid);
  const hostRows = cfg.resolver.hosts.map((raw, index) => ({ key: `host-${index}`, index, row: parseHost(raw) }));
  const routeRows = cfg.resolver.routes.map((raw, index) => ({ key: `route-${index}`, index, row: parseRoute(raw) }));
  const upstreamNames = new Set(cfg.resolver.upstreams.map((item) => item.name));
  const routeUpstreamOptions = cfg.resolver.upstreams.map((item) => ({ value: item.name, label: item.name }));
  const matchOptions = getMatchOptions(t);

  const routeUpstreamTagRender: TagRender = (props) => {
    const { label, value, closable, onClose } = props;
    const missingUpstream = !upstreamNames.has(String(value));
    return (
      <SelectTag
        color={missingUpstream ? "error" : "processing"}
        label={missingUpstream ? t("rules.upstreamInvalid", { label: String(label) }) : label}
        closable={closable}
        onClose={onClose}
      />
    );
  };

  function openImport(kind: ImportKind) {
    setImportKind(kind);
    setImportDraft("");
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
            <span className="workbenchTitle">{t("rules.title")}</span>
            <Segmented
              value={activeRuleKind}
              onChange={(value) => setActiveRuleKind(value as ImportKind)}
              options={[
                { value: "hosts", label: "Hosts" },
                { value: "routes", label: t("rules.routeRules") }
              ]}
            />
            <Tag>{t("rules.hostsCount", { count: cfg.resolver.hosts.length })}</Tag>
            <Tag>{t("rules.routesCount", { count: cfg.resolver.routes.length })}</Tag>
          </div>
          <div className="workbenchToolbarActions">
            <Button icon={<ImportOutlined />} onClick={() => openImport(activeRuleKind)}>{t("rules.bulkImport")}</Button>
            {activeRuleKind === "hosts" ? (
              <Button type="primary" icon={<PlusOutlined />} onClick={() => updateResolver({ hosts: [...cfg.resolver.hosts, formatHost({ domain: "", ips: "" })] })}>{t("rules.addRecord")}</Button>
            ) : (
              <Button type="primary" icon={<PlusOutlined />} onClick={() => updateResolver({ routes: [...cfg.resolver.routes, defaultRoute(cfg.resolver.upstreams[0]?.name || "")] })}>{t("rules.addRoute")}</Button>
            )}
          </div>
        </div>

        <main className="workbenchMain">
          {activeRuleKind === "hosts" ? (
            <div className="workbenchPanel">
              <div className="workbenchPanelHeader">
                <span className="workbenchPanelTitle">Hosts</span>
              </div>
              <div className="workbenchPanelBodyFlush">
                <Table
                  rowKey="key"
                  size="small"
                  pagination={false}
                  scroll={{ x: "max-content" }}
                  dataSource={hostRows}
                  locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("rules.noHosts")} /> }}
                  columns={[
                    {
                      title: t("rules.domain"),
                      dataIndex: ["row", "domain"],
                      render: (_value, record) => (
                        <FieldWithError error={validation.hosts[record.index]?.domain}>
                          <Input status={validation.hosts[record.index]?.domain ? "error" : undefined} value={record.row.domain} onChange={(event) => updateHost(record.index, { ...record.row, domain: event.target.value })} placeholder="example.local" />
                        </FieldWithError>
                      )
                    },
                    {
                      title: t("rules.ipAddress"),
                      dataIndex: ["row", "ips"],
                      render: (_value, record) => (
                        <FieldWithError error={validation.hosts[record.index]?.ips}>
                          <Select
                            className="workbenchInlineSelect workbenchTagsSelect"
                            status={validation.hosts[record.index]?.ips ? "error" : undefined}
                            mode="tags"
                            value={splitList(record.row.ips)}
                            onChange={(values) => updateHost(record.index, { ...record.row, ips: values.join(", ") })}
                            placeholder="127.0.0.1, ::1"
                            open={false}
                            suffixIcon={null}
                          />
                        </FieldWithError>
                      )
                    },
                    {
                      title: "",
                      width: 48,
                      align: "right",
                      render: (_value, record) => (
                        <Button icon={<DeleteOutlined />} onClick={() => removeHost(record.index)} aria-label={t("rules.deleteHost")} />
                      )
                    }
                  ]}
                />
              </div>
            </div>
          ) : (
            <div className="workbenchPanel">
              <div className="workbenchPanelHeader">
                <span className="workbenchPanelTitle">{t("rules.routeRules")}</span>
              </div>
              <div className="workbenchPanelBodyFlush">
                <Table
                  rowKey="key"
                  size="small"
                  pagination={false}
                  scroll={{ x: "max-content" }}
                  dataSource={routeRows}
                  locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("rules.noRoutes")} /> }}
                  columns={[
                    {
                      title: t("rules.matchMode"),
                      width: 150,
                      render: (_value, record) => (
                        <Select className="workbenchInlineSelect" value={record.row.match} onChange={(value) => updateRoute(record.index, { ...record.row, match: value })} options={matchOptions} />
                      )
                    },
                    {
                      title: t("rules.domain"),
                      width: 220,
                      render: (_value, record) => (
                        <FieldWithError error={validation.routes[record.index]?.domain}>
                          <Input status={validation.routes[record.index]?.domain ? "error" : undefined} value={record.row.domain} onChange={(event) => updateRoute(record.index, { ...record.row, domain: event.target.value })} placeholder="example.com" />
                        </FieldWithError>
                      )
                    },
                    {
                      title: t("rules.targetUpstream"),
                      render: (_value, record) => (
                        <FieldWithError error={validation.routes[record.index]?.upstreams}>
                          <Select
                            className="workbenchInlineSelect workbenchTagsSelect"
                            status={validation.routes[record.index]?.upstreams ? "error" : undefined}
                            mode="multiple"
                            value={record.row.upstreams}
                            onChange={(values) => updateRoute(record.index, { ...record.row, upstreams: values })}
                            options={routeUpstreamOptions}
                            tagRender={routeUpstreamTagRender}
                            placeholder={t("rules.selectUpstream")}
                          />
                        </FieldWithError>
                      )
                    },
                    {
                      title: t("rules.status"),
                      width: 180,
                      render: (_value, record) => {
                        const status = cfg.resolver.routeStatuses?.[record.index];
                        const missing = record.row.upstreams.filter((name) => !cfg.resolver.upstreams.some((item) => item.name === name));
                        const invalidReason = status?.invalidReason || (!record.row.upstreams.length ? t("rules.noUpstreamSelected") : missing.length ? t("rules.upstreamDeleted", { names: missing.join(", ") }) : "");
                        return (
                          <Space orientation="vertical" size={4}>
                            <Tag color={invalidReason ? "error" : "success"}>{invalidReason ? t("rules.invalid") : t("rules.valid")}</Tag>
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
                        <Button icon={<DeleteOutlined />} onClick={() => removeRoute(record.index)} aria-label={t("rules.deleteRoute")} />
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
        title={importKind === "hosts" ? t("rules.importHostsTitle") : t("rules.importRoutesTitle")}
        width={720}
        okText={t("rules.importCount", { count: importableItems.length })}
        okButtonProps={{ disabled: importableItems.length === 0 }}
        cancelText={t("actions.cancel")}
        onOk={commitImport}
        onCancel={closeImport}
      >
        <Space orientation="vertical" size={12} className="pageFill">
          <Input.TextArea
            value={importDraft}
            onChange={(event) => setImportDraft(event.target.value)}
            placeholder={importKind === "hosts" ? "127.0.0.1 example.local\nexample.local=127.0.0.1,::1" : "suffix:example.com=cloudflare,google"}
            spellCheck={false}
            autoSize={{ minRows: 8, maxRows: 12 }}
          />
          <Space>
            <Tag>{t("rules.parsedLines", { count: importPreview.length })}</Tag>
            <Tag color="processing">{t("rules.importableCount", { count: importableItems.length })}</Tag>
          </Space>
          <List
            size="small"
            bordered
            dataSource={importPreview}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("rules.previewEmpty")} /> }}
            renderItem={(item, index) => (
              <List.Item key={`${item.raw}-${index}`} extra={<Tag color={item.valid ? "success" : "error"}>{item.valid ? t("rules.importable") : item.reason}</Tag>}>
                <List.Item.Meta title={item.summary} description={item.raw} />
              </List.Item>
            )}
          />
        </Space>
      </Modal>
    </>
  );
}

function parseHostImport(raw: string, existing: string[], t: Translate): ImportPreviewItem[] {
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
        reason: duplicate ? t("rules.domainDuplicate") : ""
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

function parseRouteImport(raw: string, existing: string[], fallbackUpstream: string, t: Translate): ImportPreviewItem[] {
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
      summary: `${row.match}:${row.domain} -> ${row.upstreams.join(", ") || t("rules.noUpstreamSelected")}`,
      valid: !duplicate && row.upstreams.length > 0,
      reason: duplicate ? t("rules.routeDuplicate") : t("rules.upstreamMissing")
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

function SelectTag({ color, label, closable, onClose }: { color?: string; label: ReactNode; closable: boolean; onClose: () => void }) {
  function preventSelectToggle(event: MouseEvent<HTMLSpanElement>) {
    event.preventDefault();
    event.stopPropagation();
  }

  return (
    <Tag
      color={color}
      onMouseDown={preventSelectToggle}
      closable={closable}
      onClose={onClose}
      style={{ marginInlineEnd: 4 }}
    >
      {label}
    </Tag>
  );
}

function LoadingPanel() {
  const { t } = useTranslation();
  return (
    <Card title={t("rules.loadingTitle")}>
      <Typography.Text type="secondary">{t("rules.loading")}</Typography.Text>
    </Card>
  );
}

function FieldWithError({ error, children }: { error?: string; children: ReactNode }) {
  return (
    <Space direction="vertical" size={4} className="pageFill">
      {children}
      {error ? <Typography.Text type="danger">{error}</Typography.Text> : null}
    </Space>
  );
}
