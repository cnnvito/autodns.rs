import { useState } from "react";

import type { SystemDnsSettings, SystemDnsStatus } from "../shared/types";
import { SwitchField } from "../shared/ui";

type SystemDnsPageProps = {
  systemDns: SystemDnsStatus | null;
  running: boolean;
  onSystemDnsSettingsChange: (settings: SystemDnsSettings) => void;
  onApplySystemDns: () => void;
  onRestoreSystemDns: () => void;
};

export function SystemDnsPage({
  systemDns,
  running,
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
      <section className="pageStack">
        <section className="panel systemDnsHero">
          <div>
            <span className={systemDnsEnabled ? "systemDnsState active" : "systemDnsState"}>{systemDnsEnabled ? "已允许接管" : "默认关闭"}</span>
            <h2>系统 DNS 接管</h2>
            <p>{running ? "只修改选中的网络接口；执行前会再次确认，恢复时优先使用保存的原始 DNS。" : "启动本地 DNS 服务后才能接管系统 DNS。"}</p>
          </div>
          <SwitchField checked={systemDnsEnabled} onChange={(checked) => updateSystemDns({ enabled: checked })} disabled={!canManageSystemDns}>
            允许接管
          </SwitchField>
        </section>

        <section className="systemDnsSummary">
          <div className="summaryCell">
            <span>平台</span>
            <strong>{systemDns?.platform || "未知"}</strong>
            <small>{systemDns?.supported ? "支持管理" : "暂未支持"}</small>
          </div>
          <div className="summaryCell">
            <span>目标 DNS</span>
            <strong>{targetServers.length ? targetServers.join(", ") : "未设置"}</strong>
            <small>{running ? "来自本地监听地址" : "服务未启动"}</small>
          </div>
          <div className="summaryCell">
            <span>已选接口</span>
            <strong>{selectedAdapters.length}</strong>
            <small>{managedAdapters.length ? `${managedAdapters.length} 个已接管` : "尚未接管"}</small>
          </div>
        </section>

        {systemDns?.warnings.length ? (
          <div className="warningList">
            {systemDns.warnings.map((warning) => (
              <span key={warning}>{warning}</span>
            ))}
          </div>
        ) : null}

        <section className="panel configPanel">
          <header>
            <div>
              <h2>网络接口</h2>
              <p>Windows 使用稳定接口 ID；macOS 使用网络服务名。</p>
            </div>
          </header>
          <div className="adapterList">
            {(systemDns?.adapters ?? []).map((adapter) => {
              const selected = selectedAdapterIds.has(adapter.id);
              return (
              <div className={selected ? "adapterRow selected" : "adapterRow"} key={adapter.id}>
                <div className="adapterSelect">
                  <input
                    type="checkbox"
                    checked={selected}
                    disabled={!canManageSystemDns}
                    onChange={(event) => toggleSystemDnsAdapter(adapter.id, event.target.checked)}
                    aria-label={`选择 ${adapter.name}`}
                  />
                </div>
                <div className="adapterMain">
                  <strong>{adapter.name || adapter.id}</strong>
                  <span>{adapter.description || adapter.kind}</span>
                  <span>当前 DNS: {adapter.dnsServers.length ? adapter.dnsServers.join(", ") : "自动获取或未设置"}</span>
                </div>
                <div className="adapterMeta">
                  <span>{adapter.status}</span>
                  {adapter.interfaceIndex ? <span>#{adapter.interfaceIndex}</span> : null}
                  {adapter.virtualAdapter ? <span className="tag warning">虚拟</span> : null}
                  {adapter.managed ? <span className="tag live">已接管</span> : null}
                </div>
              </div>
              );
            })}
            {systemDns && systemDns.adapters.length === 0 ? <div className="emptyList">没有读取到网络接口。</div> : null}
            {!systemDns ? <div className="emptyList">正在读取网络接口。</div> : null}
          </div>
          <div className="dnsActions">
            <button onClick={() => setDnsConfirm("restore")} disabled={!systemDns?.supported || (!managedAdapters.length && !selectedAdapters.length)}>
              恢复原 DNS
            </button>
            <button className="primary" onClick={() => setDnsConfirm("apply")} disabled={!canManageSystemDns || !systemDnsEnabled || !systemDns?.settings.selectedAdapterIds.length}>
              接管选中接口
            </button>
          </div>
        </section>
      </section>

      {dnsConfirm ? (
        <div className="modalLayer" role="presentation">
          <section className="confirmDialog dnsConfirmDialog" role="dialog" aria-modal="true" aria-labelledby="dns-confirm-title">
            <header>
              <h2 id="dns-confirm-title">{dnsConfirm === "apply" ? "接管系统 DNS" : "恢复系统 DNS"}</h2>
              <p>
                {dnsConfirm === "apply"
                  ? "即将修改选中网络接口的 DNS 服务器。这个操作可能需要管理员权限。"
                  : "即将把已接管接口恢复到保存的原始 DNS；没有原始记录时会恢复为自动获取。"}
              </p>
            </header>
            <div className="confirmSummary">
              <div>
                <strong>目标 DNS</strong>
                <span>{targetServers.length ? targetServers.join(", ") : "未设置"}</span>
              </div>
              <div>
                <strong>接口</strong>
                <span>{confirmAdapters.length ? `${confirmAdapters.length} 个` : "未选择"}</span>
              </div>
            </div>
            <div className="confirmAdapterList">
              {confirmAdapters.map((adapter) => (
                <div className="confirmAdapterRow" key={adapter.id}>
                  <strong>{adapter.name || adapter.id}</strong>
                  <span>当前 DNS: {adapter.dnsServers.length ? adapter.dnsServers.join(", ") : "自动获取或未设置"}</span>
                  {adapter.originalDns?.length ? <span>原始 DNS: {adapter.originalDns.join(", ")}</span> : null}
                </div>
              ))}
            </div>
            <div className="confirmActions">
              <button onClick={() => setDnsConfirm(null)}>取消</button>
              <button className="primary" onClick={confirmSystemDnsAction} disabled={!confirmAdapters.length || (dnsConfirm === "apply" && !running)}>
                确认执行
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </>
  );
}
