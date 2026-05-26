import type { DragEvent, KeyboardEvent } from "react";
import { useEffect, useMemo, useState } from "react";
import { GripVertical, Plus, Trash2 } from "lucide-react";

import { proxyProtocolOptions, upstreamProtocolOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import { defaultPortForProtocol, defaultPortForProxy } from "../features/config/transforms";
import type { ProxyConfig, UpstreamConfig } from "../shared/types";
import { SelectField, SwitchField } from "../shared/ui";

type DragState = {
  fromIndex: number | null;
  overIndex: number | null;
  order: number[];
};

export function UpstreamsPage({ doc, onChange }: ConfigPageProps) {
  const upstreams = doc?.config.resolver.upstreams ?? [];
  const naturalOrder = useMemo(() => upstreams.map((_, index) => index), [upstreams]);
  const [dragState, setDragState] = useState<DragState>({ fromIndex: null, overIndex: null, order: [] });

  useEffect(() => {
    setDragState((current) => {
      if (current.fromIndex !== null) {
        return current;
      }
      return { fromIndex: null, overIndex: null, order: naturalOrder };
    });
  }, [naturalOrder]);

  if (!doc) {
    return <LoadingPanel />;
  }

  const currentDoc = doc;
  const cfg = currentDoc.config;
  const isDragging = dragState.fromIndex !== null;
  const visibleOrder = isDragging && dragState.order.length === cfg.resolver.upstreams.length ? dragState.order : naturalOrder;

  function updateResolver(patch: Partial<typeof cfg.resolver>) {
    onChange({ path: currentDoc.path, config: { ...cfg, resolver: { ...cfg.resolver, ...patch } } });
  }

  function updateUpstream(index: number, patch: Partial<UpstreamConfig>) {
    const upstreams = cfg.resolver.upstreams.map((item, i) => (i === index ? { ...item, ...patch } : item));
    updateResolver({ upstreams });
  }

  function updateEndpoint(index: number, patch: Partial<Pick<UpstreamConfig, "protocol" | "host" | "port" | "path">>) {
    updateUpstream(index, patch);
  }

  function addUpstream() {
    updateResolver({
      upstreams: [
        ...cfg.resolver.upstreams,
        { name: `upstream-${cfg.resolver.upstreams.length + 1}`, protocol: "udp", host: "1.1.1.1", port: "", path: "", serverName: "", proxy: "" }
      ]
    });
  }

  function removeUpstream(index: number) {
    updateResolver({ upstreams: cfg.resolver.upstreams.filter((_, i) => i !== index) });
  }

  function moveUpstream(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= cfg.resolver.upstreams.length) {
      return;
    }
    const upstreams = [...cfg.resolver.upstreams];
    [upstreams[index], upstreams[target]] = [upstreams[target], upstreams[index]];
    updateResolver({ upstreams });
  }

  function startDrag(index: number, event: DragEvent<HTMLButtonElement>) {
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", String(index));
    setDragState({ fromIndex: index, overIndex: index, order: naturalOrder });
  }

  function enterDropTarget(targetIndex: number) {
    setDragState((current) => {
      if (current.fromIndex === null || current.overIndex === targetIndex) {
        return current;
      }
      const order = current.order.length === cfg.resolver.upstreams.length ? current.order : naturalOrder;
      const fromPosition = order.indexOf(current.fromIndex);
      const targetPosition = order.indexOf(targetIndex);
      if (fromPosition < 0 || targetPosition < 0) {
        return current;
      }
      return {
        ...current,
        overIndex: targetIndex,
        order: moveOrderItem(order, fromPosition, targetPosition)
      };
    });
  }

  function allowDrop(event: DragEvent<HTMLDivElement>) {
    if (dragState.fromIndex === null) {
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
  }

  function commitDrag(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();
    const order = dragState.order.length === cfg.resolver.upstreams.length ? dragState.order : naturalOrder;
    const changed = order.some((item, index) => item !== index);
    if (changed) {
      updateResolver({ upstreams: order.map((index) => cfg.resolver.upstreams[index]) });
    }
    resetDrag();
  }

  function resetDrag() {
    setDragState({ fromIndex: null, overIndex: null, order: naturalOrder });
  }

  function handleDragHandleKeyDown(index: number, event: KeyboardEvent<HTMLButtonElement>) {
    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveUpstream(index, -1);
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveUpstream(index, 1);
    }
  }

  function updateProxy(index: number, patch: Partial<ProxyConfig>) {
    const proxies = cfg.resolver.proxies.map((item, i) => (i === index ? { ...item, ...patch } : item));
    updateResolver({ proxies });
  }

  function updateProxyEndpoint(index: number, patch: Partial<Pick<ProxyConfig, "protocol" | "host" | "port">>) {
    updateProxy(index, patch);
  }

  function addProxy() {
    updateResolver({
      proxies: [...cfg.resolver.proxies, { name: `proxy-${cfg.resolver.proxies.length + 1}`, protocol: "socks5", host: "127.0.0.1", port: "1080", username: "", password: "" }]
    });
  }

  function removeProxy(index: number) {
    const removed = cfg.resolver.proxies[index]?.name;
    const proxies = cfg.resolver.proxies.filter((_, i) => i !== index);
    const upstreams = cfg.resolver.upstreams.map((item) => (item.proxy === removed ? { ...item, proxy: "" } : item));
    const defaultProxy = cfg.resolver.defaultProxy === removed ? "" : cfg.resolver.defaultProxy;
    updateResolver({ proxies, upstreams, defaultProxy });
  }

  const proxyOptions = [{ value: "", label: "直连" }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))];

  return (
    <section className="pageStack">
      <section className="panel configPanel">
        <header>
          <div>
            <h2>上游 DNS</h2>
            <p>按列表顺序解析。</p>
          </div>
          <button className="iconTextButton" onClick={addUpstream}><Plus size={15} /> 新增上游</button>
        </header>
        <div className="logicNote">
          <strong>切换逻辑</strong>
          <span>拿到答案即返回；无答案或出错时继续下一个，全部失败后返回最后的无记录响应或 SERVFAIL。</span>
        </div>
        <div className="dataTable">
          <div className="upstreamHeaderRow">
            <span />
            <div className="tableHeader upstreamTable">
              <span>排序</span>
              <span>上游标识</span>
              <span>协议</span>
              <span>主机</span>
              <span>端口</span>
              <span>路径</span>
              <span>SNI</span>
              <span>代理</span>
              <span />
            </div>
          </div>
          {visibleOrder.map((originalIndex, visualIndex) => {
            const item = cfg.resolver.upstreams[originalIndex];
            const isDoh = item.protocol === "http" || item.protocol === "https";
            const serverNameEnabled = item.protocol === "dot" || isDoh;
            const rowClassName = [
              "upstreamDragRow",
              dragState.fromIndex === originalIndex ? "isDragging" : "",
              dragState.overIndex === originalIndex && isDragging ? "isDropTarget" : ""
            ].filter(Boolean).join(" ");
            return (
              <div
                className={rowClassName}
                key={`upstream-${originalIndex}`}
                onDragEnter={() => enterDropTarget(originalIndex)}
                onDragOver={allowDrop}
                onDrop={commitDrag}
                onDragEnd={resetDrag}
              >
                <button
                  className="iconOnlyButton dragHandle outsideDragHandle"
                  draggable
                  onDragStart={(event) => startDrag(originalIndex, event)}
                  onKeyDown={(event) => handleDragHandleKeyDown(originalIndex, event)}
                  aria-label={`拖动 ${item.name || `第 ${visualIndex + 1} 个上游`} 调整顺序`}
                  title="拖动排序，键盘可用上下方向键"
                >
                  <GripVertical size={15} />
                </button>
                <div className="tableRow upstreamTable upstreamDataRow">
                  <div className="rowOrder">
                    <strong>{visualIndex + 1}</strong>
                  </div>
                  <input value={item.name} onChange={(event) => updateUpstream(originalIndex, { name: event.target.value })} placeholder="cloudflare" />
                  <SelectField
                    value={item.protocol}
                    onChange={(value) => updateEndpoint(originalIndex, { protocol: value, port: item.port || defaultPortForProtocol(value) })}
                    options={upstreamProtocolOptions}
                  />
                  <input value={item.host} onChange={(event) => updateEndpoint(originalIndex, { host: event.target.value })} placeholder="1.1.1.1" />
                  <input value={item.port} onChange={(event) => updateEndpoint(originalIndex, { port: event.target.value })} placeholder={defaultPortForProtocol(item.protocol)} />
                  <input
                    value={isDoh ? item.path : ""}
                    onChange={(event) => updateEndpoint(originalIndex, { path: event.target.value })}
                    placeholder={isDoh ? "/dns-query" : "-"}
                    disabled={!isDoh}
                  />
                  <input
                    value={serverNameEnabled ? item.serverName : ""}
                    onChange={(event) => updateUpstream(originalIndex, { serverName: event.target.value })}
                    placeholder={serverNameEnabled ? "cloudflare-dns.com" : "-"}
                    disabled={!serverNameEnabled}
                  />
                  <SelectField value={item.proxy} onChange={(value) => updateUpstream(originalIndex, { proxy: value })} options={proxyOptions} />
                  <button className="iconOnlyButton" onClick={() => removeUpstream(originalIndex)} disabled={cfg.resolver.upstreams.length <= 1} aria-label="删除上游">
                    <Trash2 size={15} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
        <div className="resolverPolicyBar">
          <label className="compactField">
            <span>解析超时</span>
            <input value={cfg.resolver.timeout} onChange={(event) => updateResolver({ timeout: event.target.value })} placeholder="5s" />
          </label>
          <div className="resolverSwitchGroup">
            <SwitchField checked={cfg.resolver.ipv6Enabled} onChange={(checked) => updateResolver({ ipv6Enabled: checked })}>启用 IPv6 AAAA 解析</SwitchField>
          </div>
        </div>
      </section>

      <section className="panel configPanel">
        <header>
          <div>
            <h2>代理</h2>
            <p>默认代理只作用于未单独指定代理的上游。</p>
          </div>
          <button className="iconTextButton" onClick={addProxy}><Plus size={15} /> 新增代理</button>
        </header>
        <div className="proxyToolbar">
          <label className="compactField">
            <span>默认代理</span>
            <SelectField
              value={cfg.resolver.defaultProxy}
              onChange={(value) => updateResolver({ defaultProxy: value })}
              options={[{ value: "", label: "无" }, ...cfg.resolver.proxies.map((proxy) => ({ value: proxy.name, label: proxy.name }))]}
            />
          </label>
        </div>
        <div className="dataTable">
          <div className="tableHeader proxyTable">
            <span>名称</span>
            <span>协议</span>
            <span>主机</span>
            <span>端口</span>
            <span>用户名</span>
            <span>密码</span>
            <span />
          </div>
          {cfg.resolver.proxies.map((item, index) => {
            return (
              <div className="tableRow proxyTable" key={`proxy-${index}`}>
                <input value={item.name} onChange={(event) => updateProxy(index, { name: event.target.value })} placeholder="名称" />
                <SelectField
                  value={item.protocol}
                  onChange={(value) => updateProxyEndpoint(index, { protocol: value, port: item.port || defaultPortForProxy(value) })}
                  options={proxyProtocolOptions}
                />
                <input value={item.host} onChange={(event) => updateProxyEndpoint(index, { host: event.target.value })} placeholder="127.0.0.1" />
                <input value={item.port} onChange={(event) => updateProxyEndpoint(index, { port: event.target.value })} placeholder={defaultPortForProxy(item.protocol)} />
                <input value={item.username} onChange={(event) => updateProxy(index, { username: event.target.value })} placeholder="可选" />
                <input type="password" value={item.password} onChange={(event) => updateProxy(index, { password: event.target.value })} placeholder="可选" />
                <button className="iconOnlyButton" onClick={() => removeProxy(index)} aria-label="删除代理">
                  <Trash2 size={15} />
                </button>
              </div>
            );
          })}
          {cfg.resolver.proxies.length === 0 ? <p className="emptyState">还没有代理配置，上游会直接连接。</p> : null}
        </div>
      </section>
    </section>
  );
}

function moveOrderItem(order: number[], fromPosition: number, targetPosition: number): number[] {
  const next = [...order];
  const [item] = next.splice(fromPosition, 1);
  next.splice(targetPosition, 0, item);
  return next;
}

function LoadingPanel() {
  return (
    <section className="panel configPanel">
      <header>
        <div>
          <h2>上游</h2>
          <p>正在加载本地配置。</p>
        </div>
      </header>
    </section>
  );
}
