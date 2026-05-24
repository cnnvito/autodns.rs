import type { DesktopStatus } from "../shared/types";

export function OverviewPage({ status, lastStarted }: { status: DesktopStatus | null; lastStarted: string }) {
  const running = status?.running ?? false;
  const upstreams = status?.upstreamHealth ?? [];
  const healthyUpstreams = upstreams.filter((item) => item.health === "healthy").length;
  const unhealthyUpstreams = upstreams.filter((item) => item.health === "unhealthy").length;

  return (
    <section className="overviewSimple">
      <section className="panel overviewPrimary">
        <div className="overviewStatusLine">
          <span className={running ? "statusDot running" : "statusDot"} />
          <div>
            <h2>{running ? "DNS 服务运行中" : "DNS 服务已停止"}</h2>
            <p>{running ? `${status?.listen || "本地监听中"} · ${(status?.mode || "udp").toUpperCase()}` : "启动后才会接收本机 DNS 查询。"}</p>
          </div>
        </div>
      </section>

      <div className="overviewSimpleGrid">
        <Metric label="上游" value={String(status?.upstreams ?? 0)} hint={`${healthyUpstreams} 健康 / ${unhealthyUpstreams} 异常`} />
        <Metric label="路由" value={String(status?.routes ?? 0)} hint="自定义规则" />
        <Metric label="代理" value={String(status?.proxyHealth?.length ?? 0)} hint="已配置" />
        <Metric label="启动时间" value={lastStarted || "-"} hint={running ? "运行中" : "未运行"} />
      </div>
    </section>
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
