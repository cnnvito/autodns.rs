import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { dnsHistoryOverview } from "../shared/api";
import { errorMessage, formatDate, healthStateLabel } from "../shared/format";
import type { DesktopStatus, DnsHistoryOverview, UpstreamHealth } from "../shared/types";

const numberFormatter = new Intl.NumberFormat();

export function OverviewPage({
  active,
  status,
  lastStarted
}: {
  active: boolean;
  status: DesktopStatus | null;
  lastStarted: string;
}) {
  const running = status?.running ?? false;
  const upstreams = status?.upstreamHealth ?? [];
  const healthyUpstreams = upstreams.filter((item) => item.health === "healthy").length;
  const unhealthyUpstreams = upstreams.filter((item) => item.health === "unhealthy").length;
  const [overview, setOverview] = useState<DnsHistoryOverview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const refreshOverview = useCallback(async () => {
    if (document.visibilityState === "hidden") {
      return;
    }
    setLoading(true);
    setError("");
    try {
      setOverview(await dnsHistoryOverview());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!active) {
      return;
    }
    void refreshOverview();
    const timer = window.setInterval(() => {
      void refreshOverview();
    }, 60_000);
    const handleVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        void refreshOverview();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [active, refreshOverview]);

  const updatedAt = useMemo(() => formatDate(overview?.generatedAt), [overview?.generatedAt]);
  const cacheRate = overview?.total ? `${Math.round((overview.cacheHits / overview.total) * 100)}%` : "-";

  return (
    <section className="overviewSimple">
      <section className="panel overviewPrimary">
        <div className="overviewStatusHeader">
          <div className="overviewStatusLine">
            <span className={running ? "statusDot running" : "statusDot"} />
            <div>
              <h2>{running ? "DNS 服务运行中" : "DNS 服务已停止"}</h2>
              <p>{running ? `${status?.listen || "本地监听中"} · ${(status?.mode || "udp").toUpperCase()}` : "启动后才会接收本机 DNS 查询。"}</p>
            </div>
          </div>
          <button className="compactActionButton overviewRefreshButton" onClick={refreshOverview} disabled={loading}>
            <RefreshCw size={14} />
            {loading ? "刷新中" : "刷新"}
          </button>
        </div>

        <div className="overviewFacts">
          <Fact label="上游" value={String(status?.upstreams ?? 0)} hint={`${healthyUpstreams} 健康 / ${unhealthyUpstreams} 异常`} />
          <Fact label="路由" value={String(status?.routes ?? 0)} hint="自定义规则" />
          <Fact label="代理" value={String(status?.proxyHealth?.length ?? 0)} hint="已配置" />
          <Fact label="启动时间" value={lastStarted || "-"} hint={running ? "运行中" : "未运行"} />
        </div>
      </section>

      {error ? <div className="overviewError">概览统计暂不可用：{error}</div> : null}

      <div className="overviewSimpleGrid">
        <Metric label="今日解析" value={formatCount(overview?.total)} hint="最近 24 小时" />
        <Metric label="缓存命中" value={cacheRate} hint={`${formatCount(overview?.cacheHits)} 次`} />
        <Metric label="平均耗时" value={overview ? `${Math.round(overview.averageDurationMs)} ms` : "-"} hint="最近 24 小时" />
        <Metric label="失败次数" value={formatCount(overview?.failures)} hint={overview?.failures ? "需要关注" : "最近正常"} />
      </div>

      <section className="overviewPanels">
        <section className="panel overviewListPanel">
          <header>
            <div>
              <h2>常访问域名</h2>
              <p>最近 24 小时，按解析次数排序。</p>
            </div>
            <span className="overviewUpdated">{updatedAt ? `更新 ${updatedAt}` : "未更新"}</span>
          </header>
          <div className="overviewDomainList">
            {overview?.topDomains.map((item, index) => (
              <div className="overviewDomainRow" key={item.domain}>
                <span>{index + 1}</span>
                <div>
                  <strong>{item.domain}</strong>
                  <small>最近 {formatDate(item.lastSeenAt) || "-"} · 平均 {Math.round(item.averageDurationMs)} ms</small>
                </div>
                <em>{formatCount(item.count)}</em>
              </div>
            ))}
            {overview && overview.topDomains.length === 0 ? <div className="emptyState">还没有最近解析记录。</div> : null}
            {!overview && !loading ? <div className="emptyState">暂无概览统计。</div> : null}
          </div>
        </section>

        <section className="panel overviewListPanel">
          <header>
            <div>
              <h2>最近异常</h2>
              <p>只显示最近 24 小时内的失败或非正常响应。</p>
            </div>
          </header>
          <div className="overviewIssueList">
            {overview?.recentErrors.map((item) => (
              <div className="overviewIssueRow" key={item.id}>
                <div>
                  <strong>{item.domain}</strong>
                  <small>{formatDate(item.startedAt) || "-"} · {item.recordType} · {item.upstreamName || "未到达上游"}</small>
                </div>
                <span>{item.error || item.responseCode || "异常"}</span>
              </div>
            ))}
            {overview && overview.recentErrors.length === 0 ? <div className="emptyState">最近没有解析异常。</div> : null}
          </div>

          <div className="overviewUpstreamList">
            <h3>上游状态</h3>
            {upstreams.slice(0, 5).map((item) => (
              <UpstreamRow item={item} key={item.name} />
            ))}
            {upstreams.length === 0 ? <div className="emptyState">暂无上游状态。</div> : null}
          </div>
        </section>
      </section>
    </section>
  );
}

function Fact({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="overviewFact">
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{hint}</small>
    </div>
  );
}

function Metric({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{hint}</small>
    </div>
  );
}

function UpstreamRow({ item }: { item: UpstreamHealth }) {
  return (
    <div className="overviewUpstreamRow">
      <div>
        <strong>{item.name}</strong>
        <small>{item.protocol.toUpperCase()} · {item.latencyMs !== undefined ? `${item.latencyMs} ms` : "暂无延迟"}</small>
      </div>
      <span className={`healthBadge ${item.health}`}>{healthStateLabel(item.health)}</span>
    </div>
  );
}

function formatCount(value?: number): string {
  return numberFormatter.format(value ?? 0);
}
