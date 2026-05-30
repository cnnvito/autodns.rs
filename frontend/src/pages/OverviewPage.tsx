import { Alert, Badge, Button, Descriptions, Empty, List, Space, Tag, Typography } from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { dnsHistoryOverview } from "../shared/api";
import { errorMessage, formatDate, localizedMessageText, translatedHealthStateLabel } from "../shared/format";
import { LoadingOverlay } from "../shared/LoadingOverlay";
import type { DesktopStatus, DnsHistoryEntry, DnsHistoryOverview, SystemDnsStatus, UpstreamHealth } from "../shared/types";
import type { ResolvedLanguage } from "../i18n/language";

const numberFormatter = new Intl.NumberFormat();

export function OverviewPage({
  active,
  status,
  lastStarted,
  systemDns,
  systemDnsLoading,
  onNavigate,
  onApplySystemDns,
  onRestoreSystemDns,
  language
}: {
  active: boolean;
  status: DesktopStatus | null;
  lastStarted: string;
  systemDns: SystemDnsStatus | null;
  systemDnsLoading: boolean;
  onNavigate: (page: string) => void;
  onApplySystemDns: () => void;
  onRestoreSystemDns: () => void;
  language: ResolvedLanguage;
}) {
  const { t } = useTranslation();
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
      setError(errorMessage(err, (key, values) => t(key, values)));
    } finally {
      setLoading(false);
    }
  }, [t]);

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

  const updatedAt = useMemo(() => formatDate(overview?.generatedAt, language), [overview?.generatedAt, language]);
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
  const initialOverviewLoading = loading && !overview;

  return (
    <div className="pageFill desktopHome">
      <header className="desktopHomeHeader">
        <div>
          <Typography.Title level={2}>{t("overview.serviceStatus")}</Typography.Title>
        </div>
        <Button icon={<ReloadOutlined />} onClick={refreshOverview} disabled={loading}>
          {loading ? t("overview.refreshing") : t("overview.refresh")}
        </Button>
      </header>

      {error ? <Alert type="error" showIcon title={t("overview.unavailable")} description={error} /> : null}

      <div className="desktopHomeContent loadingOverlayHost" aria-busy={initialOverviewLoading}>
        <div className="homeSplitGrid">
          <section className="homePane homePaneMain">
            <header className="homePaneHeader">
              <Typography.Title level={3}><StatusTitle running={running} /></Typography.Title>
            </header>
            <Descriptions
              size="small"
              column={1}
              items={[
                { key: "listen", label: t("overview.listenAddress"), children: running ? status?.listen || t("overview.localListening") : t("overview.notStarted") },
                { key: "mode", label: t("overview.protocol"), children: running ? (status?.mode || "udp").toUpperCase() : "-" },
                { key: "started", label: t("overview.startedAt"), children: lastStarted || "-" },
                { key: "routes", label: t("overview.routesAndProxies"), children: t("overview.routesAndProxiesValue", { routes: status?.routes ?? 0, proxies: status?.proxyHealth?.length ?? 0 }) }
              ]}
            />
          </section>

          <section className="homePane homePaneSide">
            <header className="homePaneHeader">
              <Space>
                <Typography.Title level={3}>{t("overview.systemDns")}</Typography.Title>
                <Tag color={systemDnsLoading ? "warning" : systemDnsEnabled ? "success" : "default"}>
                  {systemDnsLoading ? t("overview.loading") : systemDnsEnabled ? t("overview.takeoverAllowed") : t("overview.unmanaged")}
                </Tag>
              </Space>
              <Button type="link" onClick={() => onNavigate("system-dns")}>{t("overview.details")}</Button>
            </header>
            <Descriptions
              size="small"
              column={1}
              items={[
                { key: "platform", label: t("overview.platform"), children: systemDns?.platform || t("common.unknown") },
                { key: "target", label: t("overview.targetDns"), children: targetServers.length ? targetServers.join(", ") : "-" },
                { key: "selected", label: t("overview.selectedAdapters"), children: t("overview.countItems", { count: selectedAdapters }) },
                { key: "managed", label: t("overview.managedAdapters"), children: t("overview.countItems", { count: managedAdapters }) }
              ]}
            />
            <Space className="homePaneActions">
              <Button onClick={onRestoreSystemDns} disabled={systemDnsLoading || !managedAdapters}>
                {t("overview.restore")}
              </Button>
              <Button type="primary" onClick={onApplySystemDns} disabled={systemDnsLoading || !running || !systemDnsEnabled || !selectedAdapters}>
                {t("overview.applyTakeover")}
              </Button>
            </Space>
          </section>
        </div>

        <div className="homeSplitGrid">
          <section className="homePane homePaneMain">
            <header className="homePaneHeader">
              <Typography.Title level={3}>{t("overview.recentActivity")}</Typography.Title>
              <Typography.Text type="secondary">{updatedAt ? t("overview.updatedAt", { time: updatedAt }) : t("overview.notUpdated")}</Typography.Text>
            </header>
            <Space size={12} wrap className="homeSummaryLine">
              <Tag>{t("overview.last24Hours", { count: formatCount(overview?.total) })}</Tag>
              <Tag>{t("overview.cacheHit", { rate: cacheRate })}</Tag>
              <Tag>{t("overview.average", { duration: overview ? `${Math.round(overview.averageDurationMs)} ms` : "-" })}</Tag>
              <Tag color={overview?.failures ? "error" : "success"}>{t("overview.failures", { count: formatCount(overview?.failures) })}</Tag>
            </Space>
            <List
              dataSource={recentQueries}
              locale={{ emptyText: overview ? t("overview.noRecentRecords") : t("overview.noOverview") }}
              renderItem={(item) => (
                <List.Item extra={<Typography.Text>{item.durationMs ? `${item.durationMs} ms` : "-"}</Typography.Text>}>
                  <List.Item.Meta
                    title={item.domain}
                    description={`${formatDate(item.startedAt, language) || "-"} · ${item.source === "summary" ? t("overview.frequentDomain") : item.recordType}`}
                  />
                </List.Item>
              )}
            />
          </section>

          <section className="homePane homePaneSide">
            <header className="homePaneHeader">
              <Typography.Title level={3}>{t("overview.upstreamHealth")}</Typography.Title>
              <Button type="link" onClick={() => onNavigate("upstreams")}>{t("overview.configure")}</Button>
            </header>
            {upstreams.length ? (
              <List
                size="small"
                dataSource={upstreams.slice(0, 5)}
                renderItem={(item) => <UpstreamRow item={item} />}
              />
            ) : (
              <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("overview.noUpstreamStatus")} />
            )}
            <Typography.Text type="secondary">{t("overview.healthSummary", { healthy: healthyUpstreams, unhealthy: unhealthyUpstreams })}</Typography.Text>
          </section>
        </div>

        <div className="homeSplitGrid">
          <section className="homePane homePaneMain">
            <header className="homePaneHeader">
              <Typography.Title level={3}>{t("overview.frequentDomain")}</Typography.Title>
            </header>
            <List
              dataSource={overview?.topDomains ?? []}
              locale={{ emptyText: overview && overview.topDomains.length === 0 ? t("overview.noRecentRecords") : t("overview.noOverview") }}
              renderItem={(item, index) => (
                <List.Item extra={<Typography.Text strong>{formatCount(item.count)}</Typography.Text>}>
                  <List.Item.Meta
                    avatar={<Tag>{index + 1}</Tag>}
                    title={item.domain}
                    description={t("overview.recentAverage", { time: formatDate(item.lastSeenAt, language) || "-", duration: Math.round(item.averageDurationMs) })}
                  />
                </List.Item>
              )}
            />
          </section>

          <section className="homePane homePaneSide">
            <header className="homePaneHeader">
              <Typography.Title level={3}>{t("overview.recentErrors")}</Typography.Title>
            </header>
            <List
              dataSource={overview?.recentErrors ?? []}
              locale={{ emptyText: t("overview.noRecentErrors") }}
              renderItem={(item) => (
                <List.Item extra={<Tag color="error">{historyErrorText(item, t) || item.responseCode || t("overview.abnormal")}</Tag>}>
                  <List.Item.Meta
                    title={item.domain}
                    description={`${formatDate(item.startedAt, language) || "-"} · ${item.recordType} · ${item.upstreamName || t("overview.upstreamNotReached")}`}
                  />
                </List.Item>
              )}
            />
          </section>
        </div>
        {initialOverviewLoading ? <LoadingOverlay text={t("overview.loadingOverview")} /> : null}
      </div>
    </div>
  );
}

function StatusTitle({ running }: { running: boolean }) {
  const { t } = useTranslation();
  return (
    <Space size={12}>
      <Badge status={running ? "success" : "error"} />
      <span>{running ? t("overview.serviceRunning") : t("overview.serviceStopped")}</span>
    </Space>
  );
}

function UpstreamRow({ item }: { item: UpstreamHealth }) {
  const { t } = useTranslation();
  const error = item.lastErrorMessage ? localizedMessageText(item.lastErrorMessage, t) : item.lastError;
  return (
    <List.Item extra={<Tag color={healthColor(item.health)}>{translatedHealthStateLabel(item.health, t)}</Tag>}>
      <List.Item.Meta
        title={item.name}
        description={`${item.protocol.toUpperCase()} · ${item.latencyMs !== undefined ? `${item.latencyMs} ms` : t("overview.noLatency")}${error ? ` · ${error}` : ""}`}
      />
    </List.Item>
  );
}

function historyErrorText(item: DnsHistoryEntry, translate: (key: string, values?: Record<string, unknown>) => string): string {
  return item.errorMessage ? localizedMessageText(item.errorMessage, translate) : item.error;
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
