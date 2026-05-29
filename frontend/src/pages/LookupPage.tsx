import { Alert, Button, Empty, Form, Input, Select, Space, Table, Tag, Typography } from "antd";
import { SearchOutlined } from "@ant-design/icons";
import { useState } from "react";

import { lookupDomain } from "../shared/api";
import { errorMessage } from "../shared/format";
import type { DnsLookupResult } from "../shared/types";

type SelectOption = {
  value: string;
  label: string;
};

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

  async function runLookup() {
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
    <section className="pageWorkbench">
      <Form className="workbenchToolbar" layout="inline" onFinish={runLookup}>
        <div className="workbenchToolbarMain">
          <span className="workbenchTitle">解析查询</span>
          <Form.Item className="workbenchFluidInput" label="域名" required>
            <Input value={domain} onChange={(event) => setDomain(event.target.value)} placeholder="example.com" />
          </Form.Item>
          <Form.Item className="workbenchCompactSelect" label="记录">
            <Select value={recordType} onChange={setRecordType} options={recordTypeOptions} />
          </Form.Item>
          <Tag color={running ? "success" : "warning"}>{running ? "当前解析器" : "服务未启动"}</Tag>
        </div>
        <div className="workbenchToolbarActions">
          <Button type="primary" htmlType="submit" icon={<SearchOutlined />} disabled={!running || busy || !domain.trim()}>
            {busy ? "查询中" : "查询"}
          </Button>
        </div>
      </Form>

      <div className="workbenchBody">
        <main className="workbenchMain">
          {error ? <Alert type="error" showIcon title="查询失败" description={error} style={{ marginBottom: 12 }} /> : null}
          <div className="workbenchPanel">
            <div className="workbenchPanelHeader">
              <span className="workbenchPanelTitle">答案记录</span>
              <Space>
                <Tag>{result ? `${result.answerCount} 条答案` : "等待查询"}</Tag>
                {result ? <Tag color={result.responseCode === "NOERROR" ? "success" : "warning"}>{result.responseCode}</Tag> : null}
              </Space>
            </div>
            <div className="workbenchPanelBodyFlush">
              <Table
                rowKey={(record, index) => `${record.name}-${record.recordType}-${record.value}-${index}`}
                size="small"
                pagination={false}
                dataSource={result?.records ?? []}
                locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={result ? "没有返回答案记录" : "输入域名后执行查询"} /> }}
                columns={[
                  { title: "类型", dataIndex: "recordType", width: 90, render: (value: string) => <Tag>{value}</Tag> },
                  { title: "名称", dataIndex: "name", ellipsis: true },
                  { title: "记录值", dataIndex: "value", ellipsis: true },
                  { title: "TTL", dataIndex: "ttl", width: 90, render: (value: number) => `${value} s` }
                ]}
              />
            </div>
          </div>
        </main>

        <aside className="workbenchInspector">
          <div className="workbenchInspectorSection">
            <div className="workbenchInspectorTitle">本次查询</div>
            <Space direction="vertical" size={8} className="pageFill">
              <Typography.Text type="secondary">域名</Typography.Text>
              <Typography.Text copyable={Boolean(result)}>{result?.domain || domain || "-"}</Typography.Text>
              <Typography.Text type="secondary">记录类型</Typography.Text>
              <Typography.Text>{result?.recordType || recordType}</Typography.Text>
              <Typography.Text type="secondary">响应 / 耗时</Typography.Text>
              <Typography.Text>{result ? `${result.responseCode} · ${result.durationMs} ms` : "-"}</Typography.Text>
            </Space>
          </div>
          <div className="workbenchInspectorSection">
            <div className="workbenchInspectorTitle">解析路径</div>
            <Typography.Text type="secondary">
              {running ? "查询会经过固定解析、路由规则、缓存和上游策略。" : "启动本地 DNS 服务后可以查询当前解析路径。"}
            </Typography.Text>
          </div>
        </aside>
      </div>
    </section>
  );
}
