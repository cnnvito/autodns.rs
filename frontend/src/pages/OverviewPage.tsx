import { Alert, Badge, Button, Descriptions, Empty, List, Space, Tag, Typography } from "antd";
import { ApiOutlined, DatabaseOutlined, ReloadOutlined, SearchOutlined } from "@ant-design/icons";
import { useCallback, useEffect, useMemo, useState } from "react";

import { dnsHistoryOverview } from "../shared/api";
import { errorMessage, formatDate, healthStateLabel } from "../shared/format";
import type { DesktopStatus, DnsHistoryEntry, DnsHistoryOverview, SystemDnsStatus, UpstreamHealth } from "../shared/types";

const numberFormatter = new Intl.NumberFormat();

export function OverviewPage({
  active,
  status,
  lastStarted,
  systemDns,
  systemDnsLoading,
  onNavigate,
  onApplySystemDns,
  onRestoreSystemDns
}: {
  active: boolean;
  status: DesktopStatus | null;
  lastStarted: string;
  systemDns: SystemDnsStatus | null;
  systemDnsLoading: boolean;
  onNavigate: (page: string) => void;
  onApplySystemDns: () => void;
  onRestoreSystemDns: () => void;
}) {
  const running = status?.running ?? false;
  const upstreams = status?.upstreamHealth ?? [];
  const healthyUpstreams = upstreams.filter((item) => item.health === "healthy").length;
  const unhealthyUpstreams = upstreams.filter((item) => item.health === "unhealthy").length;
  const managedAdapters = (systemDns?.adapters ?? []).filter((adapter) => adapter.managed).length;
  const selectedAdapters = systemDns?.settings.selectedAdapterIds.length ?? 0;
  const systemDnsEnabled = systemDns?.settings.enabled ?? false;
  const targetServers = systemDns?.settings.targetServers.length ? systemDns.settings.targetServers : systemDns?.localServers ?? [];
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
  const recentQueries = useMemo(() => {
    const rows: DnsHistoryEntry[] = [];
    overview?.topDomains.slice(0, 4).forEach((item, index) => {
      rows.push({
        id: index,
        startedAt: item.lastSeenAt,
        domain: item.domain,
        recordType: "-",
        source: "summary",
        routeId: 0,
        upstreamName: "",
        upstreamProtocol: "",
        durationMs: Math.round(item.averageDurationMs),
        attemptCount: 0,
        responseCode: "",
        error: ""
      });
    });
    return rows;
  }, [overview?.topDomains]);

  return (
    <Space orientation="vertical" size={16} className="pageFill desktopHome">
      <header className="desktopHomeHeader">
        <Typography.Text type="secondary"></Typography.Text>
        <Button icon={<ReloadOutlined />} onClick={refreshOverview} disabled={loading}>
          {loading ? "刷新中" : "刷新"}
        </Button>
      </header>

      {error ? <Alert type="error" showIcon title="概览统计暂不可用" description={error} /> : null}

      <div className="homeStatusGrid">
        <section className="homePane">
          <header className="homePaneHeader">
            <Typography.Title level={3}><StatusTitle running={running} /></Typography.Title>
          </header>
          <Descriptions
            size="small"
            column={1}
            items={[
              { key: "listen", label: "监听地址", children: running ? status?.listen || "本地监听中" : "未启动" },
              { key: "mode", label: "协议", children: running ? (status?.mode || "udp").toUpperCase() : "-" },
              { key: "started", label: "启动时间", children: lastStarted || "-" },
              { key: "routes", label: "路由 / 代理", children: `${status?.routes ?? 0} 条 / ${status?.proxyHealth?.length ?? 0} 个` }
            ]}
          />
        </section>

        <section className="homePane">
          <header className="homePaneHeader">
            <Space>
              <Typography.Title level={3}>系统 DNS</Typography.Title>
              <Tag color={systemDnsLoading ? "warning" : systemDnsEnabled ? "success" : "default"}>
                {systemDnsLoading ? "读取中" : systemDnsEnabled ? "允许接管" : "未接管"}
              </Tag>
            </Space>
            <Button type="link" onClick={() => onNavigate("system-dns")}>详情</Button>
          </header>
          <Descriptions
            size="small"
            column={1}
            items={[
              { key: "platform", label: "平台", children: systemDns?.platform || "未知" },
              { key: "target", label: "目标 DNS", children: targetServers.length ? targetServers.join(", ") : "-" },
              { key: "selected", label: "已选接口", children: `${selectedAdapters} 个` },
              { key: "managed", label: "已接管", children: `${managedAdapters} 个` }
            ]}
          />
          <Space className="homePaneActions">
            <Button onClick={onRestoreSystemDns} disabled={systemDnsLoading || !managedAdapters}>
              恢复
            </Button>
            <Button type="primary" onClick={onApplySystemDns} disabled={systemDnsLoading || !running || !systemDnsEnabled || !selectedAdapters}>
              应用接管
            </Button>
          </Space>
        </section>

        <section className="homePane">
          <header className="homePaneHeader">
            <Typography.Title level={3}>上游健康</Typography.Title>
            <Button type="link" onClick={() => onNavigate("upstreams")}>配置</Button>
          </header>
          {upstreams.length ? (
            <List
              size="small"
              dataSource={upstreams.slice(0, 3)}
              renderItem={(item) => <UpstreamRow item={item} />}
            />
          ) : (
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="暂无上游状态" />
          )}
          <Typography.Text type="secondary">{healthyUpstreams} 健康 / {unhealthyUpstreams} 异常</Typography.Text>
        </section>
      </div>

      <div className="homeSplitGrid">
        <section className="homePane homePaneMain">
          <header className="homePaneHeader">
            <Typography.Title level={3}>最近查询</Typography.Title>
            <Typography.Text type="secondary">{updatedAt ? `更新 ${updatedAt}` : "未更新"}</Typography.Text>
          </header>
          <Space size={12} wrap className="homeSummaryLine">
            <Tag>最近 24 小时 {formatCount(overview?.total)}</Tag>
            <Tag>缓存命中 {cacheRate}</Tag>
            <Tag>平均 {overview ? `${Math.round(overview.averageDurationMs)} ms` : "-"}</Tag>
            <Tag color={overview?.failures ? "error" : "success"}>失败 {formatCount(overview?.failures)}</Tag>
          </Space>
          <List
            dataSource={recentQueries}
            locale={{ emptyText: overview ? "还没有最近解析记录。" : "暂无概览统计。" }}
            renderItem={(item) => (
              <List.Item extra={<Typography.Text>{item.durationMs ? `${item.durationMs} ms` : "-"}</Typography.Text>}>
                <List.Item.Meta
                  title={item.domain}
                  description={`${formatDate(item.startedAt) || "-"} · ${item.source === "summary" ? "常访问域名" : item.recordType}`}
                />
              </List.Item>
            )}
          />
        </section>

        <section className="homePane homePaneSide">
          <header className="homePaneHeader">
            <Typography.Title level={3}>工作入口</Typography.Title>
          </header>
          <List
            dataSource={[
              { key: "rules", icon: <DatabaseOutlined />, title: "配置规则与上游", target: "rules" },
              { key: "lookup", icon: <SearchOutlined />, title: "测试解析与查看历史", target: "lookup" },
              { key: "system", icon: <ApiOutlined />, title: "接管或恢复系统 DNS", target: "system-dns" }
            ]}
            renderItem={(item) => (
              <List.Item className="homeTaskItem" onClick={() => onNavigate(item.target)}>
                <List.Item.Meta avatar={item.icon} title={item.title} />
              </List.Item>
            )}
          />
        </section>
      </div>

      <div className="homeSplitGrid">
        <section className="homePane homePaneMain">
          <header className="homePaneHeader">
            <Typography.Title level={3}>常访问域名</Typography.Title>
          </header>
          <List
            dataSource={overview?.topDomains ?? []}
            locale={{ emptyText: overview && overview.topDomains.length === 0 ? "还没有最近解析记录。" : "暂无概览统计。" }}
            renderItem={(item, index) => (
              <List.Item extra={<Typography.Text strong>{formatCount(item.count)}</Typography.Text>}>
                <List.Item.Meta
                  avatar={<Tag>{index + 1}</Tag>}
                  title={item.domain}
                  description={`最近 ${formatDate(item.lastSeenAt) || "-"} · 平均 ${Math.round(item.averageDurationMs)} ms`}
                />
              </List.Item>
            )}
          />
        </section>

        <section className="homePane homePaneSide">
          <header className="homePaneHeader">
            <Typography.Title level={3}>最近异常</Typography.Title>
          </header>
          <List
            dataSource={overview?.recentErrors ?? []}
            locale={{ emptyText: "最近没有解析异常。" }}
            renderItem={(item) => (
              <List.Item extra={<Tag color="error">{item.error || item.responseCode || "异常"}</Tag>}>
                <List.Item.Meta
                  title={item.domain}
                  description={`${formatDate(item.startedAt) || "-"} · ${item.recordType} · ${item.upstreamName || "未到达上游"}`}
                />
              </List.Item>
            )}
              />
        </section>
      </div>
    </Space>
  );
}

function StatusTitle({ running }: { running: boolean }) {
  return (
    <Space size={12}>
      <Badge status={running ? "success" : "error"} />
      <span>{running ? "DNS 服务运行中" : "DNS 服务已停止"}</span>
    </Space>
  );
}

function UpstreamRow({ item }: { item: UpstreamHealth }) {
  return (
    <List.Item extra={<Tag color={healthColor(item.health)}>{healthStateLabel(item.health)}</Tag>}>
      <List.Item.Meta
        title={item.name}
        description={`${item.protocol.toUpperCase()} · ${item.latencyMs !== undefined ? `${item.latencyMs} ms` : "暂无延迟"}`}
      />
    </List.Item>
  );
}

function healthColor(health: UpstreamHealth["health"]) {
  if (health === "healthy") {
    return "success";
  }
  if (health === "unhealthy") {
    return "error";
  }
  if (health === "unused") {
    return "warning";
  }
  return "default";
}

function formatCount(value?: number): string {
  return numberFormatter.format(value ?? 0);
}
