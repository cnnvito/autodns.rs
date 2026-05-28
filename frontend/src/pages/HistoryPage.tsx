import { ChevronDown, RefreshCw, Search, Trash2 } from "lucide-react";
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
  const [domain, setDomain] = useState("");
  const [filter, setFilter] = useState("");
  const [statusFilter, setStatusFilter] = useState<DnsHistoryStatusFilter>("all");
  const [historyWindow, setHistoryWindow] = useState<DnsHistoryWindow>("24h");
  const [limit, setLimit] = useState(pageSize);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const [items, setItems] = useState<DnsHistoryEntry[]>([]);
  const [topDomains, setTopDomains] = useState<DnsHistoryTopDomain[]>([]);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setFilter(domain.trim());
      setLimit(pageSize);
      setExpandedId(null);
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
    setExpandedId(null);
  }

  function updateWindow(value: DnsHistoryWindow) {
    setHistoryWindow(value);
    setLimit(pageSize);
    setExpandedId(null);
  }

  async function clearHistory() {
    if (!window.confirm("清空所有解析历史？")) {
      return;
    }
    setBusy(true);
    setError("");
    try {
      await clearDnsHistory();
      setItems([]);
      setTopDomains([]);
      setTotal(0);
      setExpandedId(null);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="historyLayout">
      <section className="panel historyDetailsPanel">
        <header>
          <div>
            <h2>解析明细</h2>
            <p>查看最近解析记录，按域名、时间和状态缩小范围。</p>
          </div>
          <div className="panelHeaderActions">
            <button onClick={refresh} disabled={busy}>
              <RefreshCw size={15} />
              刷新
            </button>
            <button onClick={clearHistory} disabled={busy}>
              <Trash2 size={15} />
              清空
            </button>
          </div>
        </header>

        <div className="historyToolbar">
          <label className="historySearchField">
            <span>域名</span>
            <div>
              <Search size={15} />
              <input value={domain} onChange={(event) => setDomain(event.target.value)} placeholder="example.com" />
            </div>
          </label>

          <div className="historyFilters">
            <SegmentedFilter
              label="状态"
              options={statusOptions}
              value={statusFilter}
              onChange={updateStatusFilter}
            />
            <SegmentedFilter
              label="时间"
              options={windowOptions}
              value={historyWindow}
              onChange={updateWindow}
            />
          </div>
        </div>

        {error ? <div className="lookupError">{error}</div> : null}

        <div className="historyList">
          <div className="historyTableHeader">
            <span>时间</span>
            <span>域名</span>
            <span>类型</span>
            <span>来源</span>
            <span>上游</span>
            <span>耗时</span>
            <span>状态</span>
          </div>
          {items.map((item) => (
            <HistoryRow
              expanded={expandedId === item.id}
              item={item}
              key={item.id}
              onToggle={() => setExpandedId((current) => (current === item.id ? null : item.id))}
            />
          ))}
          {items.length === 0 ? <div className="emptyState">{busy ? "正在加载解析历史。" : "没有符合筛选条件的解析历史。"}</div> : null}
        </div>

        {items.length < total ? (
          <div className="historyMore">
            <button onClick={() => setLimit((value) => value + pageSize)} disabled={busy}>
              加载更多
            </button>
          </div>
        ) : null}
      </section>

      <section className="panel historyTopPanel">
        <header>
          <div>
            <h2>Top 域名</h2>
            <p>跟随当前筛选条件，点击后查看明细。</p>
          </div>
        </header>

        <div className="historyTopList">
          {topDomains.map((item, index) => (
            <button className="historyTopRow" onClick={() => setDomain(item.domain)} key={item.domain}>
              <span>{index + 1}</span>
              <div>
                <strong>{item.domain}</strong>
                <small>
                  最近 {formatDate(item.lastSeenAt) || "-"} · 平均 {Math.round(item.averageDurationMs)} ms
                </small>
              </div>
              <em>{item.count}</em>
            </button>
          ))}
          {topDomains.length === 0 ? <div className="emptyState">暂无 Top 域名。</div> : null}
        </div>
      </section>
    </section>
  );
}

function SegmentedFilter<T extends string>({
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
    <div className="historySegmentedGroup">
      <span>{label}</span>
      <div className="historySegmented">
        {options.map((option) => (
          <button
            aria-pressed={value === option.value}
            className={value === option.value ? "active" : ""}
            key={option.value}
            onClick={() => onChange(option.value)}
          >
            {option.label}
          </button>
        ))}
      </div>
    </div>
  );
}

function HistoryRow({
  expanded,
  item,
  onToggle
}: {
  expanded: boolean;
  item: DnsHistoryEntry;
  onToggle: () => void;
}) {
  const source = sourceLabels[item.source] ?? (item.source || "-");
  const upstream = item.upstreamName
    ? `${item.upstreamName}${item.upstreamProtocol ? `/${item.upstreamProtocol}` : ""}`
    : "-";
  const ttl = Number.isFinite(item.minTtl) ? `${item.minTtl} s` : "-";
  const error = item.error || "-";
  const status = historyStatus(item);

  return (
    <div className="historyRowGroup">
      <button className="historyTableRow" onClick={onToggle} aria-expanded={expanded}>
        <span>{formatDate(item.startedAt)}</span>
        <strong>{item.domain}</strong>
        <span>{item.recordType}</span>
        <span className={`tag source-${item.source}`}>{source}</span>
        <span>{upstream}</span>
        <span>{item.durationMs} ms</span>
        <span className={`historyStatus ${status.kind}`}>
          <em>{status.label}</em>
          <small>{item.responseCode || "-"}</small>
          <ChevronDown size={13} />
        </span>
      </button>
      {expanded ? (
        <div className="historyRowDetails">
          <Detail label="TTL" value={ttl} />
          <Detail label="尝试" value={`${item.attemptCount}`} />
          <Detail label="路由" value={item.routeId > 0 ? `#${item.routeId}` : "默认"} />
          <Detail label="错误" value={error} wide />
        </div>
      ) : null}
    </div>
  );
}

function Detail({ label, value, wide = false }: { label: string; value: string; wide?: boolean }) {
  return (
    <div className={wide ? "historyDetail wide" : "historyDetail"}>
      <span>{label}</span>
      <strong title={value}>{value}</strong>
    </div>
  );
}

function historyStatus(item: DnsHistoryEntry): { kind: "ok" | "warn" | "error"; label: string } {
  if (item.source === "error" || item.responseCode === "SERVFAIL" || item.responseCode === "REFUSED" || item.responseCode === "INVALID") {
    return { kind: "error", label: "失败" };
  }
  if (item.responseCode === "NXDOMAIN" || item.error) {
    return { kind: "warn", label: "无记录" };
  }
  return { kind: "ok", label: "成功" };
}
