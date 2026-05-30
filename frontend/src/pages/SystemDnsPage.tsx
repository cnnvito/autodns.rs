import { Alert, Button, Checkbox, Descriptions, Empty, List, Modal, Space, Switch, Table, Tag, Typography } from "antd";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { LoadingOverlay } from "../shared/LoadingOverlay";
import { localizedMessageText } from "../shared/format";
import type { SystemDnsAdapter, SystemDnsSettings, SystemDnsStatus } from "../shared/types";

type SystemDnsPageProps = {
  systemDns: SystemDnsStatus | null;
  loading: boolean;
  running: boolean;
  embedded?: boolean;
  onSystemDnsSettingsChange: (settings: SystemDnsSettings) => void;
  onApplySystemDns: () => void;
  onRestoreSystemDns: () => void;
};

export function SystemDnsPage({
  systemDns,
  loading,
  running,
  embedded = false,
  onSystemDnsSettingsChange,
  onApplySystemDns,
  onRestoreSystemDns
}: SystemDnsPageProps) {
  const { t } = useTranslation();
  const [dnsConfirm, setDnsConfirm] = useState<"apply" | "restore" | null>(null);
  const systemDnsEnabled = systemDns?.settings.enabled ?? false;
  const canManageSystemDns = Boolean(systemDns?.supported && systemDns?.canApply && running);
  const canRestoreSystemDns = Boolean(systemDns?.supported && running);
  const targetServers = systemDns?.settings.targetServers.length ? systemDns.settings.targetServers : systemDns?.localServers ?? [];
  const selectedAdapterIds = new Set(systemDns?.settings.selectedAdapterIds ?? []);
  const selectedAdapters = (systemDns?.adapters ?? []).filter((adapter) => selectedAdapterIds.has(adapter.id));
  const managedAdapters = (systemDns?.adapters ?? []).filter((adapter) => adapter.managed);
  const confirmAdapters = dnsConfirm === "restore" ? managedAdapters.length ? managedAdapters : selectedAdapters : selectedAdapters;
  const adaptersLoaded = Boolean(systemDns && systemDns.adapters.length > 0);
  const focusedAdapter = selectedAdapters[0] ?? managedAdapters[0] ?? systemDns?.adapters[0] ?? null;
  const warnings = systemDns?.warningMessages?.length
    ? systemDns.warningMessages.map((warning) => localizedMessageText(warning, t))
    : systemDns?.warnings ?? [];
  const systemDnsError = systemDns?.lastErrorMessage
    ? localizedMessageText(systemDns.lastErrorMessage, t)
    : systemDns?.lastError || "";

  function updateSystemDns(patch: Partial<SystemDnsSettings>) {
    const current = systemDns?.settings ?? { enabled: false, targetServers: systemDns?.localServers ?? [], selectedAdapterIds: [] };
    onSystemDnsSettingsChange({ ...current, ...patch });
  }

  function toggleSystemDnsAdapter(adapterId: string, checked: boolean) {
    const current = systemDns?.settings ?? { enabled: false, targetServers: systemDns?.localServers ?? [], selectedAdapterIds: [] };
    const selected = new Set(current.selectedAdapterIds);
    if (checked) {
      selected.add(adapterId);
    } else {
      selected.delete(adapterId);
    }
    updateSystemDns({ selectedAdapterIds: Array.from(selected) });
  }

  function confirmSystemDnsAction() {
    if (dnsConfirm === "apply") {
      onApplySystemDns();
    }
    if (dnsConfirm === "restore") {
      onRestoreSystemDns();
    }
    setDnsConfirm(null);
  }

  return (
    <>
      <section className={embedded ? "systemDnsEmbedded" : "pageWorkbench"}>
        <div className="workbenchToolbar">
          <div className="workbenchToolbarMain">
            <span className="workbenchTitle">{t("systemDnsPage.title")}</span>
            <Tag color={loading ? "warning" : systemDnsEnabled ? "success" : "default"}>
              {loading ? t("systemDnsPage.loading") : systemDnsEnabled ? t("systemDnsPage.takeoverAllowed") : t("systemDnsPage.defaultOff")}
            </Tag>
            <Tag color={systemDns?.supported ? "processing" : "warning"}>{systemDns?.platform || t("systemDnsPage.unknownPlatform")}</Tag>
            <Typography.Text type="secondary" ellipsis>
              {t("systemDnsPage.targetDns")}: {targetServers.length ? targetServers.join(", ") : t("systemDnsPage.notSet")}
            </Typography.Text>
          </div>
          <div className="workbenchToolbarActions">
            <Switch checkedChildren={t("systemDnsPage.allow")} unCheckedChildren={t("systemDnsPage.off")} checked={systemDnsEnabled} onChange={(checked) => updateSystemDns({ enabled: checked })} disabled={!canManageSystemDns} />
            <Button onClick={() => setDnsConfirm("restore")} disabled={loading || !adaptersLoaded || !canRestoreSystemDns || (!managedAdapters.length && !selectedAdapters.length)}>
              {t("systemDnsPage.restoreOriginal")}
            </Button>
            <Button type="primary" onClick={() => setDnsConfirm("apply")} disabled={loading || !adaptersLoaded || !canManageSystemDns || !systemDnsEnabled || !systemDns?.settings.selectedAdapterIds.length}>
              {t("systemDnsPage.applySelected")}
            </Button>
          </div>
        </div>

        <div className="workbenchBody loadingOverlayHost" aria-busy={loading}>
          <main className="workbenchMain">
            {warnings.length ? (
              <Space orientation="vertical" size={8} className="pageFill" style={{ marginBottom: 12 }}>
                {warnings.map((warning) => (
                  <Alert key={warning} type="warning" showIcon title={warning} />
                ))}
              </Space>
            ) : null}
            {systemDnsError ? <Alert type="error" showIcon title={systemDnsError} style={{ marginBottom: 12 }} /> : null}
            <div className="workbenchStatsGrid" style={{ marginBottom: 12 }}>
              <div className="workbenchMetric">
                <strong>{systemDns?.adapters.length ?? 0}</strong>
                <span>{t("systemDnsPage.adapters")}</span>
              </div>
              <div className="workbenchMetric">
                <strong>{selectedAdapters.length}</strong>
                <span>{t("systemDnsPage.selected")}</span>
              </div>
              <div className="workbenchMetric">
                <strong>{managedAdapters.length}</strong>
                <span>{t("systemDnsPage.managed")}</span>
              </div>
            </div>
            <div className="workbenchPanel">
              <div className="workbenchPanelHeader">
                <span className="workbenchPanelTitle">{t("systemDnsPage.adapters")}</span>
              </div>
              <div className="workbenchPanelBodyFlush">
                <Table<SystemDnsAdapter>
                  rowKey="id"
                  size="small"
                  pagination={false}
                  dataSource={systemDns?.adapters ?? []}
                  locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={systemDns ? t("systemDnsPage.noAdapters") : t("systemDnsPage.noStatus")} /> }}
                  columns={[
                    {
                      title: "",
                      width: 46,
                      render: (_value, adapter) => (
                        <Checkbox
                          checked={selectedAdapterIds.has(adapter.id)}
                          disabled={!canManageSystemDns}
                          onChange={(event) => toggleSystemDnsAdapter(adapter.id, event.target.checked)}
                          aria-label={t("systemDnsPage.selectAdapter", { name: adapter.name })}
                        />
                      )
                    },
                    {
                      title: t("systemDnsPage.adapter"),
                      dataIndex: "name",
                      ellipsis: true,
                      render: (_value, adapter) => (
                        <Space>
                          <span>{adapter.name || adapter.id}</span>
                          {selectedAdapterIds.has(adapter.id) ? <Tag color="processing">{t("systemDnsPage.selected")}</Tag> : null}
                        </Space>
                      )
                    },
                    { title: t("systemDnsPage.description"), dataIndex: "description", ellipsis: true, render: (_value, adapter) => adapter.description || adapter.kind || "-" },
                    { title: t("systemDnsPage.currentDns"), dataIndex: "dnsServers", ellipsis: true, render: (value: string[]) => value.length ? value.join(", ") : t("systemDnsPage.autoDns") },
                    { title: t("systemDnsPage.status"), dataIndex: "status", width: 90, render: (value: string) => <Tag>{value}</Tag> },
                    {
                      title: t("systemDnsPage.takeover"),
                      width: 110,
                      render: (_value, adapter) => adapter.managed ? <Tag color="success">{t("systemDnsPage.managed")}</Tag> : adapter.virtualAdapter ? <Tag color="warning">{t("systemDnsPage.virtual")}</Tag> : <Tag>{t("systemDnsPage.unmanaged")}</Tag>
                    }
                  ]}
                />
              </div>
            </div>
          </main>

          <aside className="workbenchInspector">
            <div className="workbenchInspectorSection">
              <div className="workbenchInspectorTitle">{t("systemDnsPage.details")}</div>
              {focusedAdapter ? (
                <Descriptions
                  size="small"
                  column={1}
                  items={[
                    { key: "name", label: t("systemDnsPage.adapter"), children: focusedAdapter.name || focusedAdapter.id },
                    { key: "index", label: t("systemDnsPage.index"), children: focusedAdapter.interfaceIndex ? `#${focusedAdapter.interfaceIndex}` : "-" },
                    { key: "kind", label: t("systemDnsPage.type"), children: focusedAdapter.virtualAdapter ? `${focusedAdapter.kind} · ${t("systemDnsPage.virtual")}` : focusedAdapter.kind || "-" },
                    { key: "dns", label: t("systemDnsPage.currentDns"), children: focusedAdapter.dnsServers.length ? focusedAdapter.dnsServers.join(", ") : t("systemDnsPage.autoDns") },
                    { key: "original", label: t("systemDnsPage.originalDns"), children: focusedAdapter.originalDns?.length ? focusedAdapter.originalDns.join(", ") : "-" },
                    { key: "error", label: t("systemDnsPage.error"), children: adapterErrorText(focusedAdapter, t) || "-" }
                  ]}
                />
              ) : (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("systemDnsPage.selectForDetails")} />
              )}
            </div>
          </aside>
          {loading ? <LoadingOverlay text={t("systemDnsPage.loadingStatus")} /> : null}
        </div>
      </section>

      <Modal
        open={Boolean(dnsConfirm)}
        title={dnsConfirm === "apply" ? t("systemDnsPage.applyTitle") : t("systemDnsPage.restoreTitle")}
        okText={t("systemDnsPage.confirm")}
        cancelText={t("actions.cancel")}
        okButtonProps={{ disabled: loading || !adaptersLoaded || !confirmAdapters.length || (dnsConfirm === "apply" && !canManageSystemDns) || (dnsConfirm === "restore" && !canRestoreSystemDns) }}
        onOk={confirmSystemDnsAction}
        onCancel={() => setDnsConfirm(null)}
      >
        <Space orientation="vertical" size={12} className="pageFill">
          <Alert
            type={dnsConfirm === "apply" ? "warning" : "info"}
            showIcon
            title={dnsConfirm === "apply"
              ? t("systemDnsPage.applyWarning")
              : t("systemDnsPage.restoreInfo")}
          />
          <Descriptions
            size="small"
            bordered
            column={2}
            items={[
              { key: "target", label: t("systemDnsPage.targetDns"), children: targetServers.length ? targetServers.join(", ") : t("systemDnsPage.notSet") },
              { key: "adapters", label: t("systemDnsPage.adapter"), children: confirmAdapters.length ? t("systemDnsPage.adaptersCount", { count: confirmAdapters.length }) : t("systemDnsPage.noAdapterSelected") }
            ]}
          />
          <List
            size="small"
            bordered
            dataSource={confirmAdapters}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("systemDnsPage.noAdapterSelected")} /> }}
            renderItem={(adapter) => (
              <List.Item>
                <List.Item.Meta
                  title={adapter.name || adapter.id}
                  description={(
                    <Space orientation="vertical" size={2}>
                      <Typography.Text type="secondary">{t("systemDnsPage.currentDns")}: {adapter.dnsServers.length ? adapter.dnsServers.join(", ") : t("systemDnsPage.autoDns")}</Typography.Text>
                      {adapter.originalDns?.length ? <Typography.Text type="secondary">{t("systemDnsPage.originalDns")}: {adapter.originalDns.join(", ")}</Typography.Text> : null}
                    </Space>
                  )}
                />
              </List.Item>
            )}
          />
        </Space>
      </Modal>
    </>
  );
}

function adapterErrorText(adapter: SystemDnsAdapter, translate: (key: string, values?: Record<string, unknown>) => string): string {
  return adapter.lastErrorMessage ? localizedMessageText(adapter.lastErrorMessage, translate) : adapter.lastError || "";
}
