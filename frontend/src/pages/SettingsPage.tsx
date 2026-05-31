import { Alert, Button, Card, Form, Input, InputNumber, Modal, Segmented, Space, Switch, Tabs, Typography } from "antd";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpenOutlined } from "@ant-design/icons";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getLogLevelOptions, serverModeOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import type { ConfigValidation } from "../features/config/validation";
import { generateServerCertificate, loadCertificateDefaults, validateServerCertificate } from "../shared/api";
import { errorMessage } from "../shared/format";
import type { CertificateDefaults, DesktopConfig, DesktopPreferences, GeneratedCertificate, SystemDnsSettings, SystemDnsStatus } from "../shared/types";
import { SystemDnsPage } from "./SystemDnsPage";

type SelectOption = {
  value: string;
  label: string;
};

type ServerFormValues = DesktopConfig["server"];

export type SettingsSection = "general" | "service" | "system-dns" | "cache" | "health";

type SettingsPageProps = ConfigPageProps & {
  validation: ConfigValidation;
  language: string;
  languageOptions: SelectOption[];
  theme: string;
  themeOptions: SelectOption[];
  preferences: DesktopPreferences;
  running: boolean;
  busy: boolean;
  section: SettingsSection;
  systemDns: SystemDnsStatus | null;
  systemDnsLoading: boolean;
  onClearDnsCache: () => void;
  onLanguageChange: (value: string) => void;
  onThemeChange: (value: string) => void;
  onPreferencesChange: (patch: Partial<DesktopPreferences>) => void;
  onSectionChange: (section: SettingsSection) => void;
  onSystemDnsSettingsChange: (settings: SystemDnsSettings) => void;
  onApplySystemDns: () => void;
  onRestoreSystemDns: () => void;
};

const certificateListSeparators = /[\n,]+/;

export function SettingsPage({
  doc,
  onChange,
  validation,
  language,
  languageOptions,
  theme,
  themeOptions,
  preferences,
  running,
  busy,
  section,
  systemDns,
  systemDnsLoading,
  onClearDnsCache,
  onLanguageChange,
  onThemeChange,
  onPreferencesChange,
  onSectionChange,
  onSystemDnsSettingsChange,
  onApplySystemDns,
  onRestoreSystemDns
}: SettingsPageProps) {
  const { t } = useTranslation();
  const [certModalOpen, setCertModalOpen] = useState(false);
  const [certDraft, setCertDraft] = useState<CertificateDefaults | null>(null);
  const [certGenerating, setCertGenerating] = useState(false);
  const [certGenerated, setCertGenerated] = useState<GeneratedCertificate | null>(null);
  const [certError, setCertError] = useState("");
  const [tlsValidationBusy, setTlsValidationBusy] = useState(false);
  const [tlsValidationMessage, setTlsValidationMessage] = useState("");
  const [tlsValidationError, setTlsValidationError] = useState("");
  const [serverForm] = Form.useForm<ServerFormValues>();
  const serverConfig = doc?.config.server;
  const closeBehaviorOptions: SelectOption[] = [
    { value: "ask", label: t("settings.closeAsk") },
    { value: "hide", label: t("settings.closeHide") },
    { value: "quit", label: t("settings.closeQuit") }
  ];
  const settingsTabItems: Array<{ key: SettingsSection; label: string }> = [
    { key: "general", label: t("settings.tabGeneral") },
    { key: "service", label: t("settings.tabService") },
    { key: "system-dns", label: t("settings.tabSystemDns") },
    { key: "cache", label: t("settings.tabCache") },
    { key: "health", label: t("settings.tabHealth") }
  ];
  const logLevelOptions = getLogLevelOptions(t);

  useEffect(() => {
    if (serverConfig) {
      serverForm.setFieldsValue(serverConfig);
    }
  }, [serverConfig, serverForm]);

  useEffect(() => {
    if (!doc) {
      return;
    }
    const server = doc.config.server;
    const tlsEnabled = server.mode === "dot" || server.mode === "doh";
    const source = server.tlsSource || "file";
    const hasRequiredTls =
      tlsEnabled
      && (source === "inline"
        ? Boolean(server.certPem.trim() && server.keyPem.trim())
        : Boolean(server.certFile.trim() && server.keyFile.trim()));

    setTlsValidationMessage("");
    setTlsValidationError("");
    if (!hasRequiredTls) {
      setTlsValidationBusy(false);
      return;
    }

    let cancelled = false;
    setTlsValidationBusy(true);
    const timer = window.setTimeout(() => {
      validateServerCertificate(doc.config)
        .then(() => {
          if (!cancelled) {
            setTlsValidationMessage(t("settings.certificateValid"));
          }
        })
        .catch((err) => {
          if (!cancelled) {
            setTlsValidationError(errorMessage(err, t));
          }
        })
        .finally(() => {
          if (!cancelled) {
            setTlsValidationBusy(false);
          }
        });
    }, 500);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [
    doc?.config,
    doc?.config.server.mode,
    doc?.config.server.tlsSource,
    doc?.config.server.certFile,
    doc?.config.server.keyFile,
    doc?.config.server.certPem,
    doc?.config.server.keyPem,
    t
  ]);

  if (!doc) {
    return (
      <Card title={t("settings.title")}>
        <Typography.Text type="secondary">{t("settings.loading")}</Typography.Text>
      </Card>
    );
  }

  const currentDoc = doc;
  const cfg = currentDoc.config;
  const tlsFileEnabled = cfg.server.mode === "dot" || cfg.server.mode === "doh";
  const tlsSource = cfg.server.tlsSource || "file";
  const tlsInlineEnabled = tlsFileEnabled && tlsSource === "inline";
  const dohPathEnabled = cfg.server.mode === "doh";
  const listenPlaceholder = listenPlaceholderForMode(cfg.server.mode);
  const tlsSourceOptions: SelectOption[] = [
    { value: "file", label: t("settings.tlsSourceFile") },
    { value: "inline", label: t("settings.tlsSourceInline") }
  ];

  function updateConfig(next: DesktopConfig) {
    onChange({ path: currentDoc.path, config: next });
  }

  function updateServer(patch: Partial<DesktopConfig["server"]>) {
    const next = { ...cfg.server, ...patch };
    serverForm.setFieldsValue(next);
    updateConfig({ ...cfg, server: next });
  }

  function handleServerFormChange(changed: Partial<ServerFormValues>, values: ServerFormValues) {
    const next: DesktopConfig["server"] = { ...cfg.server, ...values, ...changed };
    const modeChanged = typeof changed.mode === "string";
    const tlsSourceChanged = typeof changed.tlsSource === "string";
    next.path = next.path || "/dns-query";
    next.tlsSource = next.tlsSource || "file";
    if (modeChanged && next.mode !== "doh" && next.mode !== "dot") {
      next.certFile = "";
      next.keyFile = "";
      next.certPem = "";
      next.keyPem = "";
    }
    if (tlsSourceChanged) {
      setTlsValidationMessage("");
      setTlsValidationError("");
    }
    if (modeChanged || tlsSourceChanged) {
      serverForm.setFieldsValue(next);
    }
    updateConfig({ ...cfg, server: next });
  }

  async function chooseServerFile(field: "certFile" | "keyFile", title: string) {
    if (!tlsFileEnabled) {
      return;
    }
    const selected = await open({
      title,
      multiple: false,
      directory: false
    });
    const path = Array.isArray(selected) ? selected[0] : selected;
    if (typeof path === "string" && path.length > 0) {
      updateServer({ [field]: path });
    }
  }

  async function openCertificateModal() {
    setCertError("");
    setCertGenerated(null);
    setCertModalOpen(true);
    if (!certDraft) {
      try {
        setCertDraft(await loadCertificateDefaults());
      } catch (err) {
        setCertError(errorMessage(err, t));
      }
    }
  }

  async function generateCertificate() {
    if (!certDraft || certGenerating) {
      return;
    }
    setCertGenerating(true);
    setCertError("");
    setCertGenerated(null);
    try {
      const generated = await generateServerCertificate({
        ...certDraft,
        domains: normalizeCertificateList(certDraft.domains),
        ipAddresses: normalizeCertificateList(certDraft.ipAddresses)
      });
      updateServer({ tlsSource: "file", certFile: generated.certFile, keyFile: generated.keyFile });
      setCertGenerated(generated);
      setCertModalOpen(false);
    } catch (err) {
      setCertError(errorMessage(err, t));
    } finally {
      setCertGenerating(false);
    }
  }

  function updateCache(name: keyof DesktopConfig["cache"], value: number | boolean) {
    updateConfig({
      ...cfg,
      cache: {
        ...cfg.cache,
        [name]: value
      }
    });
  }

  function updateHealthcheck(name: keyof DesktopConfig["healthcheck"], value: string | number | boolean) {
    updateConfig({
      ...cfg,
      healthcheck: {
        ...cfg.healthcheck,
        [name]: value
      }
    });
  }

  return (
    <section className="pageWorkbench">
      <div className="workbenchSettingsShell">
        <Tabs
          className="workbenchSettingsTabs"
          activeKey={section}
          onChange={(value) => onSectionChange(value as SettingsSection)}
          items={settingsTabItems}
        />
        <main className="workbenchSettingsContent">
          {section === "general" ? (
            <div className="settingRows">
              <SettingRow title={t("settings.language")}>
                <SegmentedSetting value={language} options={languageOptions} onChange={onLanguageChange} />
              </SettingRow>
              <SettingRow title={t("settings.theme")}>
                <SegmentedSetting value={theme} options={themeOptions} onChange={onThemeChange} />
              </SettingRow>
              <SettingRow title={t("settings.closeWindow")}>
                <SegmentedSetting value={preferences.closeBehavior} options={closeBehaviorOptions} onChange={(value) => onPreferencesChange({ closeBehavior: value as DesktopPreferences["closeBehavior"] })} />
              </SettingRow>
              <SettingRow title={t("settings.logLevel")}>
                <SegmentedSetting value={cfg.log.level} options={logLevelOptions} onChange={(value) => updateConfig({ ...cfg, log: { ...cfg.log, level: value } })} />
              </SettingRow>
              <SettingRow title={t("settings.startAtLogin")}>
                <Switch
                  checkedChildren={t("common.enabled")}
                  unCheckedChildren={t("common.disabled")}
                  checked={preferences.startAtLogin}
                  onChange={(checked) => onPreferencesChange({ startAtLogin: checked })}
                />
              </SettingRow>
              <SettingRow title={t("settings.historyEnabled")} description={t("settings.historyEnabledDescription")}>
                <Switch
                  checkedChildren={t("common.enabled")}
                  unCheckedChildren={t("common.disabled")}
                  checked={preferences.historyEnabled}
                  onChange={(checked) => onPreferencesChange({ historyEnabled: checked })}
                />
              </SettingRow>
            </div>
          ) : null}

          {section === "service" ? (
            <Form
              form={serverForm}
              className="settingRows serviceSettingsForm"
              initialValues={cfg.server}
              colon={false}
              requiredMark
              preserve={false}
              onValuesChange={handleServerFormChange}
            >
              <Form.Item label={t("settings.serviceMode")} name="mode">
                <SegmentedSetting options={serverModeOptions} />
              </Form.Item>
              <Form.Item
                label={t("settings.listenAddress")}
                name="listen"
                required
                validateStatus={validation.server.listen ? "error" : undefined}
                help={validation.server.listen}
                rules={[{ required: true, message: t("validation.server.listen") }]}
              >
                <Input placeholder={listenPlaceholder} />
              </Form.Item>
              {dohPathEnabled ? (
                <Form.Item
                  label={t("settings.dohPath")}
                  name="path"
                  required
                  validateStatus={validation.server.path ? "error" : undefined}
                  help={validation.server.path}
                  rules={[{ required: true, message: t("validation.server.path") }]}
                >
                  <Input placeholder="/dns-query" />
                </Form.Item>
              ) : null}
              {tlsFileEnabled ? (
                <>
                  <Form.Item label={t("settings.tlsSource")} name="tlsSource">
                    <SegmentedSetting options={tlsSourceOptions} />
                  </Form.Item>
                  {tlsInlineEnabled ? (
                    <>
                      <Form.Item
                        label={t("settings.certPem")}
                        name="certPem"
                        required
                        validateStatus={validation.server.certPem ? "error" : undefined}
                        help={validation.server.certPem}
                        rules={[{ required: true, message: t("validation.server.certPem") }]}
                      >
                        <Input.TextArea className="tlsPemTextarea" autoSize={{ minRows: 4, maxRows: 8 }} placeholder="-----BEGIN CERTIFICATE-----" />
                      </Form.Item>
                      <Form.Item
                        label={t("settings.keyPem")}
                        name="keyPem"
                        required
                        validateStatus={validation.server.keyPem ? "error" : tlsValidationError ? "error" : tlsValidationBusy ? "validating" : tlsValidationMessage ? "success" : undefined}
                        help={validation.server.keyPem || tlsValidationError || tlsValidationMessage}
                        rules={[{ required: true, message: t("validation.server.keyPem") }]}
                      >
                        <Input.TextArea className="tlsPemTextarea" autoSize={{ minRows: 4, maxRows: 8 }} placeholder="-----BEGIN PRIVATE KEY-----" />
                      </Form.Item>
                    </>
                  ) : (
                    <>
                      <Form.Item
                        label={t("settings.certFile")}
                        required
                        validateStatus={validation.server.certFile ? "error" : undefined}
                        help={validation.server.certFile}
                      >
                        <Space.Compact style={{ width: "100%" }}>
                          <Form.Item name="certFile" noStyle rules={[{ required: true, message: t("validation.server.certFile") }]}>
                            <Input placeholder="/path/to/cert.pem" />
                          </Form.Item>
                          <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("certFile", t("settings.chooseCertFile"))} aria-label={t("settings.chooseCertFile")} />
                        </Space.Compact>
                      </Form.Item>
                      <Form.Item
                        label={t("settings.keyFile")}
                        required
                        validateStatus={validation.server.keyFile ? "error" : tlsValidationError ? "error" : tlsValidationBusy ? "validating" : tlsValidationMessage ? "success" : undefined}
                        help={validation.server.keyFile || tlsValidationError || tlsValidationMessage}
                      >
                        <Space.Compact style={{ width: "100%" }}>
                          <Form.Item name="keyFile" noStyle rules={[{ required: true, message: t("validation.server.keyFile") }]}>
                            <Input placeholder="/path/to/key.pem" />
                          </Form.Item>
                          <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("keyFile", t("settings.chooseKeyFile"))} aria-label={t("settings.chooseKeyFile")} />
                        </Space.Compact>
                      </Form.Item>
                      <Form.Item label={t("settings.generateCertificate")}>
                        <Button onClick={openCertificateModal}>{t("settings.generateCertificate")}</Button>
                      </Form.Item>
                    </>
                  )}
                </>
              ) : null}
            </Form>
          ) : null}

          {section === "system-dns" ? (
            <SystemDnsPage
              embedded
              systemDns={systemDns}
              loading={systemDnsLoading}
              running={running}
              onSystemDnsSettingsChange={onSystemDnsSettingsChange}
              onApplySystemDns={onApplySystemDns}
              onRestoreSystemDns={onRestoreSystemDns}
            />
          ) : null}

          {section === "cache" ? (
            <>
              <div className="settingRows">
                <SettingRow title={t("settings.cacheEnabled")}>
                  <Switch checkedChildren={t("common.enabled")} unCheckedChildren={t("common.disabled")} checked={cfg.cache.enabled} onChange={(checked) => updateCache("enabled", checked)} />
                </SettingRow>
                <SettingRow title={t("settings.maxEntries")}>
                  <InlineNumberSetting value={cfg.cache.maxEntries} error={validation.cache.maxEntries} onChange={(value) => updateCache("maxEntries", value)} />
                </SettingRow>
                <SettingRow title={t("settings.maxEntrySize")}>
                  <InlineNumberSetting value={cfg.cache.maxEntrySize} error={validation.cache.maxEntrySize} onChange={(value) => updateCache("maxEntrySize", value)} />
                </SettingRow>
                <SettingRow title={t("settings.minTtl")}>
                  <InlineNumberSetting value={cfg.cache.minTTL} error={validation.cache.minTTL} onChange={(value) => updateCache("minTTL", value)} />
                </SettingRow>
                <SettingRow title={t("settings.maxTtl")}>
                  <InlineNumberSetting value={cfg.cache.maxTTL} error={validation.cache.maxTTL} onChange={(value) => updateCache("maxTTL", value)} />
                </SettingRow>
                <SettingRow title={t("settings.negativeTtl")}>
                  <InlineNumberSetting value={cfg.cache.negativeTTL} error={validation.cache.negativeTTL} onChange={(value) => updateCache("negativeTTL", value)} />
                </SettingRow>
              </div>
              <div className="settingActions">
                <Button onClick={onClearDnsCache} disabled={busy || !running}>{t("settings.clearNow")}</Button>
              </div>
            </>
          ) : null}

          {section === "health" ? (
            <div className="settingRows">
              <SettingRow title={t("settings.healthcheckEnabled")}>
                <Switch checkedChildren={t("common.enabled")} unCheckedChildren={t("common.disabled")} checked={cfg.healthcheck.enabled} onChange={(checked) => updateHealthcheck("enabled", checked)} />
              </SettingRow>
              <SettingRow title={t("settings.healthInterval")}>
                <ValidatedInput error={validation.healthcheck.interval}>
                  <Input status={validation.healthcheck.interval ? "error" : undefined} value={cfg.healthcheck.interval} onChange={(event) => updateHealthcheck("interval", event.target.value)} placeholder="30s" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.healthTimeout")}>
                <ValidatedInput error={validation.healthcheck.timeout}>
                  <Input status={validation.healthcheck.timeout ? "error" : undefined} value={cfg.healthcheck.timeout} onChange={(event) => updateHealthcheck("timeout", event.target.value)} placeholder="2s" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.healthDomain")}>
                <ValidatedInput error={validation.healthcheck.domain}>
                  <Input status={validation.healthcheck.domain ? "error" : undefined} value={cfg.healthcheck.domain} onChange={(event) => updateHealthcheck("domain", event.target.value)} placeholder="." />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.failureThreshold")}>
                <InlineNumberSetting value={cfg.healthcheck.failureThreshold} error={validation.healthcheck.failureThreshold} onChange={(value) => updateHealthcheck("failureThreshold", value)} />
              </SettingRow>
              <SettingRow title={t("settings.recoveryThreshold")}>
                <InlineNumberSetting value={cfg.healthcheck.recoveryThreshold} error={validation.healthcheck.recoveryThreshold} onChange={(value) => updateHealthcheck("recoveryThreshold", value)} />
              </SettingRow>
            </div>
          ) : null}
        </main>
      </div>
      <Modal
        open={certModalOpen}
        title={t("settings.certificateModalTitle")}
        okText={t("settings.generateCertificate")}
        cancelText={t("actions.cancel")}
        confirmLoading={certGenerating}
        okButtonProps={{ disabled: !certDraft }}
        onOk={generateCertificate}
        onCancel={() => setCertModalOpen(false)}
      >
        {certDraft ? (
          <Space direction="vertical" size={12} className="pageFill">
            <Alert type="info" showIcon title={t("settings.certificateTrustNotice")} />
            {certError ? <Alert type="error" showIcon title={certError} /> : null}
            {certGenerated ? (
              <Alert
                type="success"
                showIcon
                title={t("settings.certificateGenerated")}
                description={`${t("settings.caCertFile")}: ${certGenerated.caCertFile}`}
              />
            ) : null}
            <CertificateModalField title={t("settings.certificateCommonName")}>
              <Input value={certDraft.commonName} onChange={(event) => setCertDraft({ ...certDraft, commonName: event.target.value })} />
            </CertificateModalField>
            <CertificateModalField title={t("settings.certificateOrganization")}>
              <Input value={certDraft.organization} onChange={(event) => setCertDraft({ ...certDraft, organization: event.target.value })} />
            </CertificateModalField>
            <CertificateModalField title={t("settings.certificateDomains")}>
              <Input.TextArea autoSize={{ minRows: 2, maxRows: 4 }} value={certDraft.domains.join("\n")} onChange={(event) => setCertDraft({ ...certDraft, domains: splitCertificateList(event.target.value) })} />
            </CertificateModalField>
            <CertificateModalField title={t("settings.certificateIps")}>
              <Input.TextArea autoSize={{ minRows: 2, maxRows: 4 }} value={certDraft.ipAddresses.join("\n")} onChange={(event) => setCertDraft({ ...certDraft, ipAddresses: splitCertificateList(event.target.value) })} />
            </CertificateModalField>
            <CertificateModalField title={t("settings.certificateValidDays")}>
              <InputNumber className="workbenchInlineNumber" min={1} max={8250} value={certDraft.validDays} onChange={(value) => setCertDraft({ ...certDraft, validDays: value ?? 3650 })} />
            </CertificateModalField>
            <CertificateModalField title={t("settings.certificateOutputDir")}>
              <Input value={certDraft.outputDir} onChange={(event) => setCertDraft({ ...certDraft, outputDir: event.target.value })} />
            </CertificateModalField>
            <CertificateModalField title={t("settings.certificateFilePrefix")}>
              <Input value={certDraft.filePrefix} onChange={(event) => setCertDraft({ ...certDraft, filePrefix: event.target.value })} />
            </CertificateModalField>
          </Space>
        ) : (
          <Typography.Text type="secondary">{t("settings.loading")}</Typography.Text>
        )}
      </Modal>
    </section>
  );
}

function InlineNumberSetting({ value, error, onChange }: { value: number; error?: string; onChange: (value: number) => void }) {
  const { t } = useTranslation();

  return (
    <ValidatedInput error={error}>
      <InputNumber
        className="workbenchInlineNumber"
        status={error ? "error" : undefined}
        min={0}
        value={value}
        placeholder={t("settings.defaultPlaceholder")}
        onChange={(next) => onChange(typeof next === "number" && Number.isFinite(next) ? next : 0)}
      />
    </ValidatedInput>
  );
}

function splitCertificateList(value: string): string[] {
  return value.split(certificateListSeparators);
}

function normalizeCertificateList(values: string[]): string[] {
  return values.map((value) => value.trim()).filter(Boolean);
}

function listenPlaceholderForMode(mode: string): string {
  if (mode === "doh") {
    return "127.0.0.1:8443";
  }
  if (mode === "dot") {
    return "127.0.0.1:853";
  }
  return "127.0.0.1:53";
}

function SettingRow({ title, description, required, children }: { title: string; description?: string; required?: boolean; children: React.ReactNode }) {
  return (
    <div className="settingRow">
      <div className="settingRowLabel">
        <strong>{title}{required ? <span className="settingRequiredMark">*</span> : null}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      <div>{children}</div>
    </div>
  );
}

function SegmentedSetting({ value, options, onChange }: { value?: string; options: SelectOption[]; onChange?: (value: string) => void }) {
  return (
    <Segmented
      block
      className="workbenchSegmented"
      value={value}
      options={options}
      onChange={(next) => onChange?.(String(next))}
    />
  );
}

function CertificateModalField({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Space direction="vertical" size={4} className="pageFill">
      <Typography.Text strong>{title}</Typography.Text>
      {children}
    </Space>
  );
}

function ValidatedInput({ error, children }: { error?: string; children: React.ReactNode }) {
  return (
    <Space direction="vertical" size={4} className="pageFill">
      {children}
      <FieldError message={error} />
    </Space>
  );
}

function FieldError({ message }: { message?: string }) {
  return message ? <Typography.Text type="danger">{message}</Typography.Text> : null;
}
