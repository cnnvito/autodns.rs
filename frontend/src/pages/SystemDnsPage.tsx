import { Alert, Button, Checkbox, Descriptions, Empty, List, Modal, Space, Switch, Table, Tag, Typography } from "antd";
import { useState } from "react";

import { LoadingOverlay } from "../shared/LoadingOverlay";
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
  const [dnsConfirm, setDnsConfirm] = useState<"apply" | "restore" | null>(null);
  const systemDnsEnabled = systemDns?.settings.enabled ?? false;
  const canManageSystemDns = Boolean(systemDns?.supported && running);
  const targetServers = systemDns?.settings.targetServers.length ? systemDns.settings.targetServers : systemDns?.localServers ?? [];
  const selectedAdapterIds = new Set(systemDns?.settings.selectedAdapterIds ?? []);
  const selectedAdapters = (systemDns?.adapters ?? []).filter((adapter) => selectedAdapterIds.has(adapter.id));
  const managedAdapters = (systemDns?.adapters ?? []).filter((adapter) => adapter.managed);
  const confirmAdapters = dnsConfirm === "restore" ? managedAdapters.length ? managedAdapters : selectedAdapters : selectedAdapters;
  const adaptersLoaded = Boolean(systemDns && systemDns.adapters.length > 0);
  const focusedAdapter = selectedAdapters[0] ?? managedAdapters[0] ?? systemDns?.adapters[0] ?? null;

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
            <span className="workbenchTitle">系统 DNS</span>
            <Tag color={loading ? "warning" : systemDnsEnabled ? "success" : "default"}>
              {loading ? "读取中" : systemDnsEnabled ? "允许接管" : "默认关闭"}
            </Tag>
            <Tag color={systemDns?.supported ? "processing" : "warning"}>{systemDns?.platform || "未知平台"}</Tag>
            <Typography.Text type="secondary" ellipsis>
              目标 DNS: {targetServers.length ? targetServers.join(", ") : "未设置"}
            </Typography.Text>
          </div>
          <div className="workbenchToolbarActions">
            <Switch checkedChildren="允许" unCheckedChildren="关闭" checked={systemDnsEnabled} onChange={(checked) => updateSystemDns({ enabled: checked })} disabled={!canManageSystemDns} />
            <Button onClick={() => setDnsConfirm("restore")} disabled={loading || !adaptersLoaded || !systemDns?.supported || (!managedAdapters.length && !selectedAdapters.length)}>
              恢复原 DNS
            </Button>
            <Button type="primary" onClick={() => setDnsConfirm("apply")} disabled={loading || !adaptersLoaded || !canManageSystemDns || !systemDnsEnabled || !systemDns?.settings.selectedAdapterIds.length}>
              接管选中接口
            </Button>
          </div>
        </div>

        <div className="workbenchBody loadingOverlayHost" aria-busy={loading}>
          <main className="workbenchMain">
            {systemDns?.warnings.length ? (
              <Space orientation="vertical" size={8} className="pageFill" style={{ marginBottom: 12 }}>
                {systemDns.warnings.map((warning) => (
                  <Alert key={warning} type="warning" showIcon title={warning} />
                ))}
              </Space>
            ) : null}
            <div className="workbenchStatsGrid" style={{ marginBottom: 12 }}>
              <div className="workbenchMetric">
                <strong>{systemDns?.adapters.length ?? 0}</strong>
                <span>网络接口</span>
              </div>
              <div className="workbenchMetric">
                <strong>{selectedAdapters.length}</strong>
                <span>已选择</span>
              </div>
              <div className="workbenchMetric">
                <strong>{managedAdapters.length}</strong>
                <span>已接管</span>
              </div>
            </div>
            <div className="workbenchPanel">
              <div className="workbenchPanelHeader">
                <span className="workbenchPanelTitle">网络接口</span>
              </div>
              <div className="workbenchPanelBodyFlush">
                <Table<SystemDnsAdapter>
                  rowKey="id"
                  size="small"
                  pagination={false}
                  dataSource={systemDns?.adapters ?? []}
                  locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={systemDns ? "未读取到网络接口" : "还没有系统 DNS 状态"} /> }}
                  columns={[
                    {
                      title: "",
                      width: 46,
                      render: (_value, adapter) => (
                        <Checkbox
                          checked={selectedAdapterIds.has(adapter.id)}
                          disabled={!canManageSystemDns}
                          onChange={(event) => toggleSystemDnsAdapter(adapter.id, event.target.checked)}
                          aria-label={`选择 ${adapter.name}`}
                        />
                      )
                    },
                    {
                      title: "接口",
                      dataIndex: "name",
                      ellipsis: true,
                      render: (_value, adapter) => (
                        <Space>
                          <span>{adapter.name || adapter.id}</span>
                          {selectedAdapterIds.has(adapter.id) ? <Tag color="processing">已选择</Tag> : null}
                        </Space>
                      )
                    },
                    { title: "描述", dataIndex: "description", ellipsis: true, render: (_value, adapter) => adapter.description || adapter.kind || "-" },
                    { title: "当前 DNS", dataIndex: "dnsServers", ellipsis: true, render: (value: string[]) => value.length ? value.join(", ") : "自动获取或未设置" },
                    { title: "状态", dataIndex: "status", width: 90, render: (value: string) => <Tag>{value}</Tag> },
                    {
                      title: "接管",
                      width: 110,
                      render: (_value, adapter) => adapter.managed ? <Tag color="success">已接管</Tag> : adapter.virtualAdapter ? <Tag color="warning">虚拟</Tag> : <Tag>未接管</Tag>
                    }
                  ]}
                />
              </div>
            </div>
          </main>

          <aside className="workbenchInspector">
            <div className="workbenchInspectorSection">
              <div className="workbenchInspectorTitle">接口详情</div>
              {focusedAdapter ? (
                <Descriptions
                  size="small"
                  column={1}
                  items={[
                    { key: "name", label: "接口", children: focusedAdapter.name || focusedAdapter.id },
                    { key: "index", label: "索引", children: focusedAdapter.interfaceIndex ? `#${focusedAdapter.interfaceIndex}` : "-" },
                    { key: "kind", label: "类型", children: focusedAdapter.virtualAdapter ? `${focusedAdapter.kind} · 虚拟` : focusedAdapter.kind || "-" },
                    { key: "dns", label: "当前 DNS", children: focusedAdapter.dnsServers.length ? focusedAdapter.dnsServers.join(", ") : "自动获取或未设置" },
                    { key: "original", label: "原始 DNS", children: focusedAdapter.originalDns?.length ? focusedAdapter.originalDns.join(", ") : "-" },
                    { key: "error", label: "错误", children: focusedAdapter.lastError || "-" }
                  ]}
                />
              ) : (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="选择接口后查看详情" />
              )}
            </div>
          </aside>
          {loading ? <LoadingOverlay text="正在读取系统 DNS 状态" /> : null}
        </div>
      </section>

      <Modal
        open={Boolean(dnsConfirm)}
        title={dnsConfirm === "apply" ? "接管系统 DNS" : "恢复系统 DNS"}
        okText="确认执行"
        cancelText="取消"
        okButtonProps={{ disabled: loading || !adaptersLoaded || !confirmAdapters.length || (dnsConfirm === "apply" && !running) }}
        onOk={confirmSystemDnsAction}
        onCancel={() => setDnsConfirm(null)}
      >
        <Space orientation="vertical" size={12} className="pageFill">
          <Alert
            type={dnsConfirm === "apply" ? "warning" : "info"}
            showIcon
            title={dnsConfirm === "apply"
              ? "即将修改选中网络接口的 DNS 服务器。这个操作可能需要管理员权限。"
              : "将恢复已接管接口的 DNS 设置；没有原始记录时恢复为自动获取。"}
          />
          <Descriptions
            size="small"
            bordered
            column={2}
            items={[
              { key: "target", label: "目标 DNS", children: targetServers.length ? targetServers.join(", ") : "未设置" },
              { key: "adapters", label: "接口", children: confirmAdapters.length ? `${confirmAdapters.length} 个` : "未选择" }
            ]}
          />
          <List
            size="small"
            bordered
            dataSource={confirmAdapters}
            locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="未选择接口" /> }}
            renderItem={(adapter) => (
              <List.Item>
                <List.Item.Meta
                  title={adapter.name || adapter.id}
                  description={(
                    <Space orientation="vertical" size={2}>
                      <Typography.Text type="secondary">当前 DNS: {adapter.dnsServers.length ? adapter.dnsServers.join(", ") : "自动获取或未设置"}</Typography.Text>
                      {adapter.originalDns?.length ? <Typography.Text type="secondary">原始 DNS: {adapter.originalDns.join(", ")}</Typography.Text> : null}
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
