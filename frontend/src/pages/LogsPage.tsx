import { formatDate, logLevelLabel } from "../shared/format";
import type { LogEntry } from "../shared/types";

export function LogsPage({ logs }: { logs: LogEntry[] }) {
  return (
    <section className="panel logPanel">
      <header>
        <div>
          <h2>最近日志</h2>
          <p>{logs.length ? `${logs.length} 条缓存日志` : "暂无日志"}</p>
        </div>
      </header>
      <div className="logs">
        {logs.length === 0 ? <p className="emptyState">服务运行后，这里会显示最近的运行日志。</p> : null}
        {logs.slice(-160).reverse().map((entry, index) => (
          <LogRow entry={entry} key={`${entry.time}-${entry.message}-${index}`} />
        ))}
      </div>
    </section>
  );
}

function LogRow({ entry }: { entry: LogEntry }) {
  const level = entry.level.toLowerCase();
  return (
    <article className="logRow">
      <span className={`level ${level}`}>{logLevelLabel(level)}</span>
      <time>{formatDate(entry.time) || "--"}</time>
      <p>{entry.message}</p>
    </article>
  );
}
