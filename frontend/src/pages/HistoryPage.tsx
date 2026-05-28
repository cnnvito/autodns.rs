import { RefreshCw, Search, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { clearDnsHistory, dnsHistoryTopDomains, listDnsHistory } from "../shared/api";
import { errorMessage, formatDate } from "../shared/format";
import type { DnsHistoryEntry, DnsHistoryTopDomain } from "../shared/types";

const pageSize = 100;

const sourceLabels: Record<string, string> = {
  upstream: "上游",
  cache: "缓存",
  hosts: "Hosts",
  local: "本地",
  error: "失败"
};

export function HistoryPage() {
  const [domain, setDomain] = useState("");
  const [filter, setFilter] = useState("");
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
        listDnsHistory(filter, limit, 0),
        dnsHistoryTopDomains(20)
      ]);
      setItems(history.items);
      setTotal(history.total);
      setTopDomains(nextTopDomains);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }, [filter, limit]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

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
            <p>查看最近解析记录，按域名缩小范围。</p>
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
            <span>TTL</span>
            <span>响应</span>
            <span>错误</span>
          </div>
          {items.map((item) => (
            <HistoryRow item={item} key={item.id} />
          ))}
          {items.length === 0 ? <div className="emptyState">{busy ? "正在加载解析历史。" : "还没有解析历史。"}</div> : null}
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
            <p>按解析次数排序，点击后查看明细。</p>
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

function HistoryRow({ item }: { item: DnsHistoryEntry }) {
  const source = sourceLabels[item.source] ?? (item.source || "-");
  const upstream = item.upstreamName
    ? `${item.upstreamName}${item.upstreamProtocol ? `/${item.upstreamProtocol}` : ""}`
    : "-";
  const ttl = Number.isFinite(item.minTtl) ? `${item.minTtl} s` : "-";
  const error = item.error || "-";

  return (
    <div className="historyTableRow">
      <span>{formatDate(item.startedAt)}</span>
      <strong>{item.domain}</strong>
      <span>{item.recordType}</span>
      <span className={`tag source-${item.source}`}>{source}</span>
      <span>{upstream}</span>
      <span>{item.durationMs} ms</span>
      <span>{ttl}</span>
      <span className={item.responseCode === "NOERROR" ? "historyCode ok" : "historyCode"}>{item.responseCode}</span>
      <span title={error}>{error}</span>
    </div>
  );
}
