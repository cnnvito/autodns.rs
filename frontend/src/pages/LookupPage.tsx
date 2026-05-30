import { Alert, Button, Empty, Form, Input, List, Select, Space, Tag, Typography } from "antd";
import { SearchOutlined } from "@ant-design/icons";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { lookupDomain } from "../shared/api";
import { errorMessage } from "../shared/format";
import { LoadingOverlay } from "../shared/LoadingOverlay";
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
  const { t } = useTranslation();
  const [domain, setDomain] = useState("");
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
      setError(errorMessage(err, (key, values) => t(key, values)));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="pageWorkbench">
      <Form className="workbenchToolbar lookupToolbar" onFinish={runLookup}>
        <Space.Compact className="lookupSearchControl">
          <Select
            className="lookupTypeSelect"
            value={recordType}
            onChange={setRecordType}
            options={recordTypeOptions}
            aria-label={t("lookup.recordTypeLabel")}
          />
          <Input
            className="lookupDomainInput"
            value={domain}
            onChange={(event) => setDomain(event.target.value)}
            placeholder="example.com"
            aria-label={t("lookup.domainLabel")}
          />
          <Button className="lookupSearchButton" type="primary" htmlType="submit" icon={<SearchOutlined />} loading={busy} disabled={!running || !domain.trim()}>
            {t("lookup.query")}
          </Button>
        </Space.Compact>
      </Form>

      <div className="lookupBody">
        <main className="workbenchMain">
          {error ? <Alert type="error" showIcon title={t("lookup.failed")} description={error} style={{ marginBottom: 12 }} /> : null}
          <div className="workbenchPanel">
            <div className="workbenchPanelHeader">
              <span className="workbenchPanelTitle">{t("lookup.answers")}</span>
              <Space>
                <Tag>{result ? t("lookup.answerCount", { count: result.answerCount }) : t("lookup.waiting")}</Tag>
                {result ? <Tag color={result.responseCode === "NOERROR" ? "success" : "warning"}>{result.responseCode}</Tag> : null}
              </Space>
            </div>
            <div className="workbenchPanelBodyFlush lookupAnswerPanelBody loadingOverlayHost" aria-busy={busy}>
              <List
                className="lookupAnswerList"
                rowKey={(record) => `${record.name}-${record.recordType}-${record.ttl}-${record.value}`}
                dataSource={result?.records ?? []}
                locale={{ emptyText: <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={result ? t("lookup.emptyAfterQuery") : t("lookup.emptyBeforeQuery")} /> }}
                renderItem={(record) => (
                  <List.Item className="lookupAnswerItem" extra={<Tag>{record.ttl} s</Tag>}>
                    <List.Item.Meta
                      avatar={<Tag color="processing">{record.recordType}</Tag>}
                      title={<Typography.Text ellipsis title={record.name}>{record.name}</Typography.Text>}
                      description={<Typography.Text className="lookupAnswerValue" copyable ellipsis title={record.value}>{record.value}</Typography.Text>}
                    />
                  </List.Item>
                )}
              />
              {busy ? <LoadingOverlay compact text={t("lookup.loading")} /> : null}
            </div>
          </div>
        </main>
      </div>
    </section>
  );
}
