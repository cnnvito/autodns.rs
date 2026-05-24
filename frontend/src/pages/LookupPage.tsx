import { Search } from "lucide-react";
import { useState } from "react";

import { lookupDomain } from "../shared/api";
import { errorMessage } from "../shared/format";
import type { DnsLookupResult } from "../shared/types";
import { SelectField, type SelectOption } from "../shared/ui";

const recordTypeOptions: SelectOption[] = [
  { value: "A", label: "A" },
  { value: "AAAA", label: "AAAA" },
  { value: "CNAME", label: "CNAME" },
  { value: "MX", label: "MX" },
  { value: "TXT", label: "TXT" },
  { value: "NS", label: "NS" },
  { value: "SOA", label: "SOA" },
  { value: "HTTPS", label: "HTTPS" }
];

export function LookupPage({ running }: { running: boolean }) {
  const [domain, setDomain] = useState("example.com");
  const [recordType, setRecordType] = useState("A");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<DnsLookupResult | null>(null);
  const [error, setError] = useState("");

  async function runLookup(event?: React.FormEvent<HTMLFormElement>) {
    event?.preventDefault();
    if (!running || !domain.trim()) {
      return;
    }
    setBusy(true);
    setError("");
    try {
      setResult(await lookupDomain(domain.trim(), recordType));
    } catch (err) {
      setResult(null);
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="pageStack">
      <section className="panel lookupPanel">
        <header>
          <div>
            <h2>域名解析查询</h2>
            <p>{running ? "通过当前运行中的解析器查询，结果会经过 hosts、路由、缓存和上游策略。" : "服务启动后可查询当前解析路径的结果。"}</p>
          </div>
        </header>

        <form className="lookupForm" onSubmit={runLookup}>
          <label className="lookupDomainField">
            <span>域名</span>
            <input value={domain} onChange={(event) => setDomain(event.target.value)} placeholder="example.com" />
          </label>
          <label className="lookupTypeField">
            <span>记录</span>
            <SelectField value={recordType} onChange={setRecordType} options={recordTypeOptions} />
          </label>
          <button className="primary lookupSubmit" type="submit" disabled={!running || busy || !domain.trim()}>
            <Search size={15} />
            {busy ? "查询中" : "查询"}
          </button>
        </form>

        {error ? <div className="lookupError">{error}</div> : null}

        {result ? (
          <section className="lookupResult">
            <div className="lookupSummary">
              <div>
                <span>域名</span>
                <strong>{result.domain}</strong>
              </div>
              <div>
                <span>记录</span>
                <strong>{result.recordType}</strong>
              </div>
              <div>
                <span>响应</span>
                <strong>{result.responseCode}</strong>
              </div>
              <div>
                <span>耗时</span>
                <strong>{result.durationMs} ms</strong>
              </div>
            </div>

            {result.records.length ? (
              <div className="lookupRecordList">
                {result.records.map((record, index) => (
                  <div className="lookupRecord" key={`${record.name}-${record.recordType}-${record.value}-${index}`}>
                    <div>
                      <span className="tag">{record.recordType}</span>
                      <strong>{record.value}</strong>
                    </div>
                    <span>{record.name} · TTL {record.ttl}</span>
                  </div>
                ))}
              </div>
            ) : (
              <div className="emptyState">没有返回答案记录。</div>
            )}
          </section>
        ) : null}
      </section>
    </section>
  );
}
