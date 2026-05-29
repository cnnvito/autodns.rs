import { App as AntdApp, Button, Descriptions, Empty, Input, List, Segmented, Space, Table, Tag, Typography, type TableColumnsType } from "antd";
import { DeleteOutlined, ReloadOutlined, SearchOutlined } from "@ant-design/icons";
import { useCallback, useEffect, useState } from "react";

import { clearDnsHistory, dnsHistoryTopDomains, listDnsHistory } from "../shared/api";
import { errorMessage, formatDate } from "../shared/format";
import type {
  DnsHistoryEntry,
  DnsHistoryStatusFilter,
  DnsHistoryTopDomain,
  DnsHistoryWindow
} from "../shared/types";

const pageSize = 100;

const sourceLabels: Record<string, string> = {
  upstream: "上游",
  cache: "缓存",
  hosts: "Hosts",
  local: "本地",
  error: "失败"
};

const statusOptions: Array<{ value: DnsHistoryStatusFilter; label: string }> = [
  { value: "all", label: "全部" },
  { value: "errors", label: "异常" }
];

const windowOptions: Array<{ value: DnsHistoryWindow; label: string }> = [
  { value: "1h", label: "1 小时" },
  { value: "24h", label: "24 小时" },
  { value: "all", label: "全部时间" }
];

export function HistoryPage() {
  const { modal } = AntdApp.useApp();
  const [domain, setDomain] = useState("");
  const [filter, setFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<DnsHistoryStatusFilter>("all");
  const [historyWindow, setHistoryWindow] = useState<DnsHistoryWindow>("24h");
  const [limit, setLimit] = useState(pageSize);
  const [items, setItems] = useState<DnsHistoryEntry[]>([]);
  const [topDomains, setTopDomains] = useState<DnsHistoryTopDomain[]>([]);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setFilter(domain.trim());
      setLimit(pageSize);
    }, 300);
    return () => window.clearTimeout(timer);
  }, [domain]);

  const refresh = useCallback(async () => {
    setBusy(true);
    setError("");
    try {
      const [history, nextTopDomains] = await Promise.all([
        listDnsHistory(filter, limit, 0, statusFilter, historyWindow),
        dnsHistoryTopDomains(20, filter, statusFilter, historyWindow)
      ]);
      setItems(history.items);
      setTotal(history.total);
      setTopDomains(nextTopDomains);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }, [filter, historyWindow, limit, statusFilter]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  function updateStatusFilter(value: DnsHistoryStatusFilter) {
    setStatusFilter(value);
    setLimit(pageSize);
  }

  function updateWindow(value: DnsHistoryWindow) {
    setHistoryWindow(value);
    setLimit(pageSize);
  }

  async function clearHistory() {
    modal.confirm({
      title: "清空解析历史",
      content: "清空后无法从本地历史中恢复这些记录。",
      okText: "清空",
      okButtonProps: { danger: true },
      cancelText: "取消",
      onOk: async () => {
        setBusy(true);
        setError("");
        try {
          await clearDnsHistory();
          setItems([]);
          setTopDomains([]);
          setTotal(0);
        } catch (err) {
          setError(errorMessage(err));
        } finally {
          setBusy(false);
        }
      }
    });
  }

  const columns: TableColumnsType<DnsHistoryEntry> = [
    { title: "时间", dataIndex: "startedAt", width: 160, render: (value: string) => formatDate(value) || "-" },
    { title: "域名", dataIndex: "domain", ellipsis: true },
    { title: "类型", dataIndex: "recordType", width: 76 },
    { title: "来源", dataIndex: "source", width: 86, render: (value: string) => <Tag>{sourceLabels[value] ?? (value || "-")}</Tag> },
    {
      title: "上游",
      dataIndex: "upstreamName",
      width: 150,
      ellipsis: true,
      render: (_: string, item) => item.upstreamName ? `${item.upstreamName}${item.upstreamProtocol ? `/${item.upstreamProtocol}` : ""}` : "-"
    },
    { title: "耗时", dataIndex: "durationMs", width: 86, render: (value: number) => `${value} ms` },
    { title: "状态", width: 104, render: (_, item) => <HistoryStatus item={item} /> }
  ];
  const focusedItem = items[0] ?? null;

  return (
    <section className="pageWorkbench">
      <div className="workbenchToolbar">
        <div className="workbenchToolbarMain">
          <span className="workbenchTitle">解析历史</span>
            <Input
              className="workbenchFluidInput"
              prefix={<SearchOutlined />}
              value={domain}
              onChange={(event) => setDomain(event.target.value)}
              placeholder="example.com"
            />
            <FilterControl label="状态" options={statusOptions} value={statusFilter} onChange={updateStatusFilter} />
            <FilterControl label="时间" options={windowOptions} value={historyWindow} onChange={updateWindow} />
          <Tag>{total} 条记录</Tag>
        </div>
        <div className="workbenchToolbarActions">
          <Button icon={<ReloadOutlined />} onClick={refresh} disabled={busy}>
            刷新
          </Button>
          <Button icon={<DeleteOutlined />} onClick={clearHistory} disabled={busy} danger>
            清空
          </Button>
        </div>
      </div>

      <div className="workbenchBody">
        <main className="workbenchMain">
          {error ? <Typography.Text type="danger">{error}</Typography.Text> : null}

          <div className="workbenchPanel">
            <div className="workbenchPanelHeader">
              <span className="workbenchPanelTitle">解析明细</span>
              <Typography.Text type="secondary">显示 {items.length} / {total} 条</Typography.Text>
            </div>
            <div className="workbenchPanelBodyFlush">
          <Table
            rowKey="id"
            size="small"
            loading={busy}
            columns={columns}
            dataSource={items}
            pagination={false}
            expandable={{
              expandedRowRender: (item) => (
                <Descriptions
                  size="small"
                  column={4}
                  items={[
                    { key: "ttl", label: "TTL", children: Number.isFinite(item.minTtl) ? `${item.minTtl} s` : "-" },
                    { key: "attempts", label: "尝试", children: item.attemptCount },
                    { key: "route", label: "路由", children: item.routeId > 0 ? `#${item.routeId}` : "默认" },
                    { key: "error", label: "错误", children: item.error || "-" }
                  ]}
                />
              )
            }}
            locale={{ emptyText: busy ? "正在加载解析历史。" : "没有符合筛选条件的解析历史。" }}
          />
            </div>
          </div>

          {items.length < total ? (
            <Button style={{ marginTop: 16 }} onClick={() => setLimit((value) => value + pageSize)} disabled={busy}>
              加载更多
            </Button>
          ) : null}
        </main>

        <aside className="workbenchInspector">
          <div className="workbenchInspectorSection">
            <div className="workbenchInspectorTitle">最新记录</div>
            {focusedItem ? (
              <Descriptions
                size="small"
                column={1}
                items={[
                  { key: "domain", label: "域名", children: focusedItem.domain },
                  { key: "recordType", label: "类型", children: focusedItem.recordType },
                  { key: "source", label: "来源", children: sourceLabels[focusedItem.source] ?? focusedItem.source },
                  { key: "upstream", label: "上游", children: focusedItem.upstreamName ? `${focusedItem.upstreamName}${focusedItem.upstreamProtocol ? `/${focusedItem.upstreamProtocol}` : ""}` : "-" },
                  { key: "duration", label: "耗时", children: `${focusedItem.durationMs} ms` },
                  { key: "error", label: "错误", children: focusedItem.error || "-" }
                ]}
              />
            ) : (
              <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无记录" />
            )}
          </div>

          <div className="workbenchInspectorSection">
            <div className="workbenchInspectorTitle">Top 域名</div>
          <List
            size="small"
            dataSource={topDomains}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无 Top 域名" /> }}
            renderItem={(item, index) => (
              <List.Item onClick={() => setDomain(item.domain)} style={{ cursor: "pointer" }} extra={<Typography.Text strong>{item.count}</Typography.Text>}>
                <List.Item.Meta
                  avatar={<Tag>{index + 1}</Tag>}
                  title={item.domain}
                  description={`最近 ${formatDate(item.lastSeenAt) || "-"} · 平均 ${Math.round(item.averageDurationMs)} ms`}
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
  const status = historyStatus(item);
  return <Tag color={status.color}>{status.label}</Tag>;
}

function historyStatus(item: DnsHistoryEntry): { color: string; label: string } {
  if (item.source === "error" || item.responseCode === "SERVFAIL" || item.responseCode === "REFUSED" || item.responseCode === "INVALID") {
    return { color: "error", label: "失败" };
  }
  if (item.responseCode === "NXDOMAIN" || item.error) {
    return { color: "warning", label: "无记录" };
  }
  return { color: "success", label: "成功" };
}
