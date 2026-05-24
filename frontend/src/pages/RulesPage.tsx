import { useState } from "react";
import { FileInput, Plus, Trash2, X } from "lucide-react";

import { matchOptions } from "../features/config/options";
import { defaultRoute, formatHost, formatRoute, parseHost, parseRoute } from "../features/config/transforms";
import { SelectField } from "../shared/ui";
import type { ConfigPageProps } from "../features/config/doc";

type ImportKind = "hosts" | "routes";

type ImportPreviewItem = {
  raw: string;
  value: string;
  summary: string;
  valid: boolean;
  reason: string;
};

export function RulesPage({ doc, onChange }: ConfigPageProps) {
  const [importKind, setImportKind] = useState<ImportKind | null>(null);
  const [importDraft, setImportDraft] = useState("");

  if (!doc) {
    return <LoadingPanel />;
  }

  const currentDoc = doc;
  const cfg = currentDoc.config;

  function updateResolver(patch: Partial<typeof cfg.resolver>) {
    onChange({ path: currentDoc.path, config: { ...cfg, resolver: { ...cfg.resolver, ...patch } } });
  }

  function updateHost(index: number, row: ReturnType<typeof parseHost>) {
    const hosts = cfg.resolver.hosts.map((item, i) => (i === index ? formatHost(row) : item));
    updateResolver({ hosts });
  }

  function removeHost(index: number) {
    updateResolver({ hosts: cfg.resolver.hosts.filter((_, i) => i !== index) });
  }

  function updateRoute(index: number, row: ReturnType<typeof parseRoute>) {
    const routes = cfg.resolver.routes.map((item, i) => (i === index ? formatRoute(row) : item));
    updateResolver({ routes });
  }

  function removeRoute(index: number) {
    updateResolver({ routes: cfg.resolver.routes.filter((_, i) => i !== index) });
  }

  function appendRouteUpstream(index: number, name: string) {
    if (!name) {
      return;
    }
    const row = parseRoute(cfg.resolver.routes[index]);
    if (row.upstreams.includes(name)) {
      return;
    }
    updateRoute(index, { ...row, upstreams: [...row.upstreams, name] });
  }

  function removeRouteUpstream(index: number, name: string) {
    const row = parseRoute(cfg.resolver.routes[index]);
    updateRoute(index, { ...row, upstreams: row.upstreams.filter((item) => item !== name) });
  }

  function routeUpstreamOptions(selected: string[]) {
    const available = cfg.resolver.upstreams
      .filter((item) => !selected.includes(item.name))
      .map((item) => ({ value: item.name, label: item.name }));
    if (available.length === 0) {
      return [{ value: "", label: selected.length ? "已选择全部上游" : "暂无上游" }];
    }
    return [{ value: "", label: "添加上游" }, ...available];
  }

  const importPreview = importKind === "hosts"
    ? parseHostImport(importDraft, cfg.resolver.hosts)
    : importKind === "routes"
      ? parseRouteImport(importDraft, cfg.resolver.routes, cfg.resolver.upstreams[0]?.name || "")
      : [];
  const importableItems = importPreview.filter((item) => item.valid);

  function openImport(kind: ImportKind) {
    setImportKind(kind);
    setImportDraft(kind === "hosts" ? "127.0.0.1 example.local\n::1 ipv6.local" : `suffix:example.com=${cfg.resolver.upstreams[0]?.name || "upstream-1"}`);
  }

  function closeImport() {
    setImportKind(null);
    setImportDraft("");
  }

  function commitImport() {
    if (!importKind || importableItems.length === 0) {
      return;
    }
    if (importKind === "hosts") {
      updateResolver({ hosts: [...cfg.resolver.hosts, ...importableItems.map((item) => item.value)] });
    } else {
      updateResolver({ routes: [...cfg.resolver.routes, ...importableItems.map((item) => item.value)] });
    }
    closeImport();
  }

  return (
    <>
      <section className="pageStack">
        <section className="panel configPanel">
        <header>
          <div>
            <h2>固定解析</h2>
            <p>命中后直接返回这里的 IP，不再请求上游。</p>
          </div>
          <div className="panelHeaderActions">
            <button className="iconTextButton" onClick={() => openImport("hosts")}>
              <FileInput size={15} /> 批量导入
            </button>
            <button className="iconTextButton" onClick={() => updateResolver({ hosts: [...cfg.resolver.hosts, "example.local=127.0.0.1"] })}>
              <Plus size={15} /> 新增解析
            </button>
          </div>
        </header>
        <div className="dataTable">
          <div className="tableHeader hostTable">
            <span>域名</span>
            <span>IP 地址</span>
            <span />
          </div>
          {cfg.resolver.hosts.map((raw, index) => {
            const row = parseHost(raw);
            return (
              <div className="tableRow hostTable" key={`host-${index}`}>
                <input value={row.domain} onChange={(event) => updateHost(index, { ...row, domain: event.target.value })} placeholder="example.local" />
                <input value={row.ips} onChange={(event) => updateHost(index, { ...row, ips: event.target.value })} placeholder="127.0.0.1, ::1" />
                <button className="iconOnlyButton" onClick={() => removeHost(index)} aria-label="删除固定解析">
                  <Trash2 size={15} />
                </button>
              </div>
            );
          })}
          {cfg.resolver.hosts.length === 0 ? <p className="emptyState">还没有固定解析记录。</p> : null}
        </div>
        </section>

        <section className="panel configPanel">
        <header>
          <div>
            <h2>路由规则</h2>
            <p>命中的域名走指定上游；未命中时按上游顺序解析。</p>
          </div>
          <div className="panelHeaderActions">
            <button className="iconTextButton" onClick={() => openImport("routes")}>
              <FileInput size={15} /> 批量导入
            </button>
            <button className="iconTextButton" onClick={() => updateResolver({ routes: [...cfg.resolver.routes, defaultRoute(cfg.resolver.upstreams[0]?.name || "")] })}>
              <Plus size={15} /> 新增路由
            </button>
          </div>
        </header>
        <div className="dataTable">
          <div className="tableHeader routeTable">
            <span>匹配方式</span>
            <span>域名</span>
            <span>目标上游</span>
            <span>状态</span>
            <span />
          </div>
          {cfg.resolver.routes.map((raw, index) => {
            const row = parseRoute(raw);
            const status = cfg.resolver.routeStatuses?.[index];
            const missing = row.upstreams.filter((name) => !cfg.resolver.upstreams.some((item) => item.name === name));
            const invalidReason = status?.invalidReason || (!row.upstreams.length ? "未选择上游" : missing.length ? `引用的上游已删除：${missing.join(", ")}` : "");
            return (
              <div className={invalidReason ? "tableRow routeTable invalidRouteRow" : "tableRow routeTable"} key={`route-${index}`}>
                <SelectField value={row.match} onChange={(value) => updateRoute(index, { ...row, match: value })} options={matchOptions} />
                <input value={row.domain} onChange={(event) => updateRoute(index, { ...row, domain: event.target.value })} placeholder="example.com" />
                <div className="routeUpstreamPicker">
                  <div className="routeUpstreamChips">
                    {row.upstreams.map((name) => {
                      const missingUpstream = !cfg.resolver.upstreams.some((item) => item.name === name);
                      return (
                        <span className={missingUpstream ? "routeUpstreamChip missing" : "routeUpstreamChip"} key={name}>
                          {name}{missingUpstream ? "（已失效）" : ""}
                          <button onClick={() => removeRouteUpstream(index, name)} aria-label={`移除 ${name}`}>
                            <X size={12} />
                          </button>
                        </span>
                      );
                    })}
                    {row.upstreams.length === 0 ? <span className="routeUpstreamEmpty">未选择上游</span> : null}
                  </div>
                  <SelectField value="" onChange={(value) => appendRouteUpstream(index, value)} options={routeUpstreamOptions(row.upstreams)} />
                </div>
                <span className={invalidReason ? "routeState invalid" : "routeState active"}>{invalidReason ? "已失效" : "有效"}</span>
                <button className="iconOnlyButton" onClick={() => removeRoute(index)} aria-label="删除路由">
                  <Trash2 size={15} />
                </button>
                {invalidReason ? <p className="routeInvalidReason">{invalidReason}</p> : null}
              </div>
            );
          })}
          {cfg.resolver.routes.length === 0 ? <p className="emptyState">还没有自定义路由，默认会按上游顺序解析。</p> : null}
        </div>
        </section>
      </section>

      {importKind ? (
        <div className="modalLayer" role="presentation">
          <section className="confirmDialog importDialog" role="dialog" aria-modal="true" aria-labelledby="import-dialog-title">
            <header>
              <h2 id="import-dialog-title">{importKind === "hosts" ? "批量导入固定解析" : "批量导入路由规则"}</h2>
              <p>{importKind === "hosts" ? "支持 hosts 文件格式，也支持 domain=ip1,ip2。" : "支持 suffix:domain=upstream1,upstream2；省略匹配方式时默认 suffix。"}</p>
            </header>
            <textarea
              className="importTextarea"
              value={importDraft}
              onChange={(event) => setImportDraft(event.target.value)}
              spellCheck={false}
            />
            <div className="importSummary">
              <span>{importPreview.length} 行已解析</span>
              <strong>{importableItems.length} 条可导入</strong>
            </div>
            <div className="importPreviewList">
              {importPreview.map((item, index) => (
                <article className={item.valid ? "importPreviewRow" : "importPreviewRow invalid"} key={`${item.raw}-${index}`}>
                  <div>
                    <strong>{item.summary}</strong>
                    <span>{item.raw}</span>
                  </div>
                  <small>{item.valid ? "可导入" : item.reason}</small>
                </article>
              ))}
              {importPreview.length === 0 ? <div className="emptyList">粘贴内容后会在这里预览。</div> : null}
            </div>
            <div className="confirmActions">
              <button onClick={closeImport}>取消</button>
              <button className="primary" onClick={commitImport} disabled={importableItems.length === 0}>导入 {importableItems.length} 条</button>
            </div>
          </section>
        </div>
      ) : null}
    </>
  );
}

function parseHostImport(raw: string, existing: string[]): ImportPreviewItem[] {
  const seen = new Set(existing.map((item) => parseHost(item).domain.toLowerCase()).filter(Boolean));
  return contentLines(raw).flatMap((line) => {
    const parsedRows = parseHostImportLine(line);
    if (parsedRows.length === 0) {
      return [];
    }
    return parsedRows.map((parsed) => {
      const key = parsed.domain.toLowerCase();
      const duplicate = seen.has(key);
      if (!duplicate) {
        seen.add(key);
      }
      const value = formatHost({ domain: parsed.domain, ips: parsed.ips.join(", ") });
      return {
        raw: line,
        value,
        summary: `${parsed.domain} -> ${parsed.ips.join(", ")}`,
        valid: !duplicate,
        reason: duplicate ? "域名已存在，已跳过" : ""
      };
    });
  });
}

function parseHostImportLine(line: string): { domain: string; ips: string[] }[] {
  const equalParts = line.split("=");
  if (equalParts.length === 2) {
    const domain = equalParts[0].trim();
    const ips = splitList(equalParts[1]);
    return domain && ips.length ? [{ domain, ips }] : [];
  }
  const parts = line.split(/\s+/).filter(Boolean);
  if (parts.length < 2) {
    return [];
  }
  const [first, second, ...rest] = parts;
  if (looksLikeIp(first)) {
    return [second, ...rest]
      .filter((domain) => !looksLikeIp(domain))
      .map((domain) => ({ domain, ips: [first] }));
  }
  if (looksLikeIp(second)) {
    return [{ domain: first, ips: [second, ...rest.filter(looksLikeIp)] }];
  }
  return [];
}

function parseRouteImport(raw: string, existing: string[], fallbackUpstream: string): ImportPreviewItem[] {
  const seen = new Set(existing.map(routeKey));
  return contentLines(raw).flatMap((line) => {
    const row = parseRouteImportLine(line, fallbackUpstream);
    if (!row) {
      return [];
    }
    const value = formatRoute(row);
    const key = routeKey(value);
    const duplicate = seen.has(key);
    if (!duplicate) {
      seen.add(key);
    }
    return [{
      raw: line,
      value,
      summary: `${row.match}:${row.domain} -> ${row.upstreams.join(", ") || "未选择上游"}`,
      valid: !duplicate && row.upstreams.length > 0,
      reason: duplicate ? "路由已存在，已跳过" : "未指定上游"
    }];
  });
}

function parseRouteImportLine(line: string, fallbackUpstream: string): ReturnType<typeof parseRoute> | null {
  if (line.includes(":") && line.includes("=")) {
    return parseRoute(line);
  }
  const [domainPart, upstreamPart = ""] = line.split("=");
  const tokens = domainPart.trim().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) {
    return null;
  }
  let match = "suffix";
  let domain = tokens[0];
  if (tokens[0] === "exact" || tokens[0] === "suffix" || tokens[0] === "wildcard") {
    match = tokens[0];
    domain = tokens[1] || "";
  }
  const upstreams = splitList(upstreamPart || tokens.slice(match === tokens[0] ? 2 : 1).join(","));
  return { match, domain, upstreams: upstreams.length ? upstreams : fallbackUpstream ? [fallbackUpstream] : [] };
}

function contentLines(raw: string): string[] {
  return raw
    .split(/\r?\n/)
    .map((line) => line.replace(/\s+#.*$/, "").trim())
    .filter(Boolean);
}

function splitList(raw: string): string[] {
  return raw
    .split(/[,\s]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function looksLikeIp(value: string): boolean {
  return /^(\d{1,3}\.){3}\d{1,3}$/.test(value) || /^[0-9a-f:]+$/i.test(value);
}

function routeKey(raw: string): string {
  const row = parseRoute(raw);
  return `${row.match}:${row.domain.toLowerCase()}=${row.upstreams.join(",")}`;
}

function LoadingPanel() {
  return (
    <section className="panel configPanel">
      <header>
        <div>
          <h2>规则</h2>
          <p>正在加载本地配置。</p>
        </div>
      </header>
    </section>
  );
}
