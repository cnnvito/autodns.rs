import { App as AntdApp, Button, Descriptions, Empty, Input, List, Segmented, Space, Table, Tag, Typography, type TableColumnsType } from "antd";
import { DeleteOutlined, ReloadOutlined, SearchOutlined } from "@ant-design/icons";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ResolvedLanguage } from "../i18n/language";

import { clearDnsHistory, dnsHistoryTopDomains, listDnsHistory } from "../shared/api";
import { errorMessage, formatDate, localizedMessageText } from "../shared/format";
import type {
  DnsHistoryEntry,
  DnsHistoryStatusFilter,
  DnsHistoryTopDomain,
  DnsHistoryWindow
} from "../shared/types";

const defaultHistoryPageSize = 20;
const historyPageSizeOptions = [20, 50, 100];

export function HistoryPage({ language }: { language: ResolvedLanguage }) {
  const { t } = useTranslation();
  const { modal } = AntdApp.useApp();
  const [domain, setDomain] = useState("");
  const [filter, setFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<DnsHistoryStatusFilter>("all");
  const [historyWindow, setHistoryWindow] = useState<DnsHistoryWindow>("24h");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(defaultHistoryPageSize);
  const [items, setItems] = useState<DnsHistoryEntry[]>([]);
  const [topDomains, setTopDomains] = useState<DnsHistoryTopDomain[]>([]);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setFilter(domain.trim());
      setPage(1);
    }, 300);
    return () => window.clearTimeout(timer);
  }, [domain]);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      const [history, nextTopDomains] = await Promise.all([
        listDnsHistory(filter, pageSize, (page - 1) * pageSize, statusFilter, historyWindow),
        dnsHistoryTopDomains(20, filter, statusFilter, historyWindow)
      ]);
      setItems(history.items);
      setTotal(history.total);
      setTopDomains(nextTopDomains);
    } catch (err) {
      setError(errorMessage(err, (key, values) => t(key, values)));
    } finally {
      setBusy(false);
    }
  }, [filter, historyWindow, page, statusFilter, t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  function updateStatusFilter(value: DnsHistoryStatusFilter) {
    setStatusFilter(value);
    setPage(1);
  }

  function updateWindow(value: DnsHistoryWindow) {
    setHistoryWindow(value);
    setPage(1);
  }

  function updatePage(nextPage: number, nextPageSize: number) {
    setPageSize(nextPageSize);
    setPage(nextPageSize === pageSize ? nextPage : 1);
  }

  async function clearHistory() {
    modal.confirm({
      title: t("history.clearTitle"),
      content: t("history.clearDescription"),
      okText: t("history.clear"),
      okButtonProps: { danger: true },
      cancelText: t("actions.cancel"),
      onOk: async () => {
        setBusy(true);
        setError("");
        try {
          await clearDnsHistory();
          setItems([]);
          setTopDomains([]);
          setTotal(0);
          setPage(1);
        } catch (err) {
          setError(errorMessage(err, (key, values) => t(key, values)));
        } finally {
          setBusy(false);
        }
      }
    });
  }

  const sourceLabels: Record<string, string> = {
    upstream: t("history.source.upstream"),
    cache: t("history.source.cache"),
    hosts: t("history.source.hosts"),
    local: t("history.source.local"),
    error: t("history.source.error")
  };
  const statusOptions: Array<{ value: DnsHistoryStatusFilter; label: string }> = [
    { value: "all", label: t("history.all") },
    { value: "errors", label: t("history.errors") }
  ];
  const windowOptions: Array<{ value: DnsHistoryWindow; label: string }> = [
    { value: "1h", label: t("history.oneHour") },
    { value: "24h", label: t("history.twentyFourHours") },
    { value: "all", label: t("history.allTime") }
  ];
  const columns: TableColumnsType<DnsHistoryEntry> = [
    { title: t("history.columns.time"), dataIndex: "startedAt", width: 160, render: (value: string) => formatDate(value, language) || "-" },
    { title: t("history.columns.domain"), dataIndex: "domain", ellipsis: true },
    { title: t("history.columns.type"), dataIndex: "recordType", width: 76 },
    { title: t("history.columns.source"), dataIndex: "source", width: 86, render: (value: string) => <Tag>{sourceLabels[value] ?? (value || "-")}</Tag> },
    {
      title: t("history.columns.upstream"),
      dataIndex: "upstreamName",
      width: 150,
      ellipsis: true,
      render: (_: string, item) => item.upstreamName ? `${item.upstreamName}${item.upstreamProtocol ? `/${item.upstreamProtocol}` : ""}` : "-"
    },
    { title: t("history.columns.duration"), dataIndex: "durationMs", width: 86, render: (value: number) => `${value} ms` },
    { title: t("history.columns.status"), width: 104, render: (_, item) => <HistoryStatus item={item} /> }
  ];
  const pageStart = total === 0 ? 0 : (page - 1) * pageSize + 1;
  const pageEnd = Math.min(page * pageSize, total);

  return (
    <section className="pageWorkbench">
      <div className="workbenchToolbar">
        <div className="workbenchToolbarMain">
          <span className="workbenchTitle">{t("history.title")}</span>
            <Input
              className="workbenchFluidInput"
              prefix={<SearchOutlined />}
              value={domain}
              onChange={(event) => setDomain(event.target.value)}
              placeholder="example.com"
            />
            <FilterControl label={t("history.status")} options={statusOptions} value={statusFilter} onChange={updateStatusFilter} />
            <FilterControl label={t("history.time")} options={windowOptions} value={historyWindow} onChange={updateWindow} />
          <Tag>{t("history.recordsCount", { count: total })}</Tag>
        </div>
        <div className="workbenchToolbarActions">
          <Button icon={<ReloadOutlined />} onClick={refresh} disabled={busy}>
            {t("history.refresh")}
          </Button>
          <Button icon={<DeleteOutlined />} onClick={clearHistory} disabled={busy} danger>
            {t("history.clear")}
          </Button>
        </div>
      </div>

      <div className="workbenchBody">
        <main className="workbenchMain">
          {error ? <Typography.Text type="danger">{error}</Typography.Text> : null}

          <div className="workbenchPanel">
            <div className="workbenchPanelHeader">
              <span className="workbenchPanelTitle">{t("history.details")}</span>
              <Typography.Text type="secondary">{total ? `${pageStart}-${pageEnd} / ${total}` : t("history.zeroRecords")}</Typography.Text>
            </div>
            <div className="workbenchPanelBodyFlush">
          <Table
            rowKey="id"
            size="small"
            loading={busy}
            columns={columns}
            dataSource={items}
            pagination={{
              current: page,
              pageSize,
              total,
              pageSizeOptions: historyPageSizeOptions,
              showSizeChanger: true,
              showLessItems: true,
              showTotal: (value) => t("history.total", { count: value }),
              onChange: updatePage
            }}
            expandable={{
              expandedRowRender: (item) => (
                <Descriptions
                  size="small"
                  column={4}
                  items={[
                    { key: "ttl", label: "TTL", children: Number.isFinite(item.minTtl) ? `${item.minTtl} s` : "-" },
                    { key: "attempts", label: t("history.detail.attempts"), children: item.attemptCount },
                    { key: "route", label: t("history.detail.route"), children: item.routeId > 0 ? `#${item.routeId}` : t("history.detail.defaultRoute") },
                    { key: "error", label: t("history.detail.error"), children: historyErrorText(item, t) || "-" }
                  ]}
                />
              )
            }}
            locale={{ emptyText: busy ? t("history.loading") : t("history.emptyFiltered") }}
          />
            </div>
          </div>
        </main>

        <aside className="workbenchInspector">
          <div className="workbenchInspectorSection">
            <div className="workbenchInspectorTitle">{t("history.topDomains")}</div>
          <List
            size="small"
            dataSource={topDomains}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("history.noTopDomains")} /> }}
            renderItem={(item, index) => (
              <List.Item onClick={() => setDomain(item.domain)} style={{ cursor: "pointer" }} extra={<Typography.Text strong>{item.count}</Typography.Text>}>
                <List.Item.Meta
                  avatar={<Tag>{index + 1}</Tag>}
                  title={item.domain}
                  description={t("history.recentAverage", { time: formatDate(item.lastSeenAt, language) || "-", duration: Math.round(item.averageDurationMs) })}
                />
              </List.Item>
            )}
          />
          </div>
        </aside>
      </div>
    </section>
  );
}

function FilterControl<T extends string>({
  label,
  options,
  value,
  onChange
}: {
  label: string;
  options: Array<{ value: T; label: string }>;
  value: T;
  onChange: (value: T) => void;
}) {
  return (
    <Space className="workbenchInlineFilter" size={6}>
      <Typography.Text type="secondary">{label}</Typography.Text>
      <Segmented value={value} onChange={(next) => onChange(next as T)} options={options.map((option) => ({ label: option.label, value: option.value }))} />
    </Space>
  );
}

function HistoryStatus({ item }: { item: DnsHistoryEntry }) {
  const { t } = useTranslation();
  const status = historyStatus(item, t);
  return <Tag color={status.color}>{status.label}</Tag>;
}

function historyStatus(item: DnsHistoryEntry, t: (key: string, values?: Record<string, unknown>) => string): { color: string; label: string } {
  if (item.source === "error" || item.responseCode === "SERVFAIL" || item.responseCode === "REFUSED" || item.responseCode === "INVALID") {
    return { color: "error", label: t("history.result.failed") };
  }
  if (item.responseCode === "NXDOMAIN" || item.error) {
    return { color: "warning", label: t("history.result.noRecord") };
  }
  return { color: "success", label: t("history.result.success") };
}

function historyErrorText(item: DnsHistoryEntry, translate: (key: string, values?: Record<string, unknown>) => string): string {
  return item.errorMessage ? localizedMessageText(item.errorMessage, translate) : item.error;
}
