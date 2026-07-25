import { Alert, Button, Input, InputNumber, Modal, Segmented, Space, Switch, Tabs, Typography } from "antd";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpenOutlined } from "@ant-design/icons";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getLogLevelOptions, serverModeOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import type { ConfigValidation } from "../features/config/validation";
import { generateServerCertificate, loadCertificateDefaults, validateServerCertificate } from "../shared/api";
import { errorMessage } from "../shared/format";
import { CommitOnBlurInput } from "../shared/CommitOnBlurInput";
import { HintTooltip } from "../shared/HintTooltip";
import { LoadingPanel } from "../shared/LoadingPanel";
import { ValidatedField } from "../shared/ValidatedField";
import type { CertificateDefaults, DesktopConfig, DesktopPreferences, GeneratedCertificate, SystemDnsSettings, SystemDnsStatus } from "../shared/types";
import { SystemDnsPage } from "./SystemDnsPage";

type SelectOption = {
  value: string;
  label: string;
};

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
    return <LoadingPanel title={t("settings.title")} text={t("settings.loading")} />;
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
  const tlsStatusHint = tlsValidationError
    ? null
    : tlsValidationBusy
      ? <Typography.Text type="secondary">{t("busy.processing")}</Typography.Text>
      : tlsValidationMessage
        ? <Typography.Text type="success">{tlsValidationMessage}</Typography.Text>
        : null;

  function updateConfig(next: DesktopConfig) {
    onChange({ path: currentDoc.path, config: next });
  }

  function updateServer(patch: Partial<DesktopConfig["server"]>) {
    const next = { ...cfg.server, ...patch };
    if (patch.mode !== undefined) {
      next.path = next.path || "/dns-query";
      next.tlsSource = next.tlsSource || "file";
      if (next.mode !== "doh" && next.mode !== "dot") {
        next.certFile = "";
        next.keyFile = "";
        next.certPem = "";
        next.keyPem = "";
      }
    }
    if (patch.tlsSource !== undefined) {
      setTlsValidationMessage("");
      setTlsValidationError("");
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
            <div className="settingRows">
              <SettingRow title={t("settings.serviceMode")}>
                <SegmentedSetting value={cfg.server.mode} options={serverModeOptions} onChange={(value) => updateServer({ mode: value })} />
              </SettingRow>
              <SettingRow title={t("settings.listenAddress")} required>
                <ValidatedField error={validation.server.listen}>
                  <Input
                    status={validation.server.listen ? "error" : undefined}
                    value={cfg.server.listen}
                    onChange={(event) => updateServer({ listen: event.target.value })}
                    placeholder={listenPlaceholder}
                  />
                </ValidatedField>
              </SettingRow>
              {dohPathEnabled ? (
                <SettingRow title={t("settings.dohPath")} required>
                  <ValidatedField error={validation.server.path}>
                    <Input
                      status={validation.server.path ? "error" : undefined}
                      value={cfg.server.path}
                      onChange={(event) => updateServer({ path: event.target.value })}
                      placeholder="/dns-query"
                    />
                  </ValidatedField>
                </SettingRow>
              ) : null}
              {tlsFileEnabled ? (
                <>
                  <SettingRow title={t("settings.tlsSource")}>
                    <SegmentedSetting value={tlsSource} options={tlsSourceOptions} onChange={(value) => updateServer({ tlsSource: value })} />
                  </SettingRow>
                  {tlsInlineEnabled ? (
                    <>
                      <SettingRow title={t("settings.certPem")} required>
                        <ValidatedField error={validation.server.certPem}>
                          <Input.TextArea
                            className="tlsPemTextarea"
                            status={validation.server.certPem ? "error" : undefined}
                            autoSize={{ minRows: 4, maxRows: 8 }}
                            value={cfg.server.certPem}
                            onChange={(event) => updateServer({ certPem: event.target.value })}
                            placeholder="-----BEGIN CERTIFICATE-----"
                          />
                        </ValidatedField>
                      </SettingRow>
                      <SettingRow title={t("settings.keyPem")} required>
                        <ValidatedField error={validation.server.keyPem || tlsValidationError}>
                          <Input.TextArea
                            className="tlsPemTextarea"
                            status={validation.server.keyPem || tlsValidationError ? "error" : undefined}
                            autoSize={{ minRows: 4, maxRows: 8 }}
                            value={cfg.server.keyPem}
                            onChange={(event) => updateServer({ keyPem: event.target.value })}
                            placeholder="-----BEGIN PRIVATE KEY-----"
                          />
                          {tlsStatusHint}
                        </ValidatedField>
                      </SettingRow>
                    </>
                  ) : (
                    <>
                      <SettingRow title={t("settings.certFile")} required>
                        <ValidatedField error={validation.server.certFile}>
                          <Space.Compact style={{ width: "100%" }}>
                            <Input
                              status={validation.server.certFile ? "error" : undefined}
                              value={cfg.server.certFile}
                              onChange={(event) => updateServer({ certFile: event.target.value })}
                              placeholder="/path/to/cert.pem"
                            />
                            <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("certFile", t("settings.chooseCertFile"))} aria-label={t("settings.chooseCertFile")} />
                          </Space.Compact>
                        </ValidatedField>
                      </SettingRow>
                      <SettingRow title={t("settings.keyFile")} required>
                        <ValidatedField error={validation.server.keyFile || tlsValidationError}>
                          <Space.Compact style={{ width: "100%" }}>
                            <Input
                              status={validation.server.keyFile || tlsValidationError ? "error" : undefined}
                              value={cfg.server.keyFile}
                              onChange={(event) => updateServer({ keyFile: event.target.value })}
                              placeholder="/path/to/key.pem"
                            />
                            <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("keyFile", t("settings.chooseKeyFile"))} aria-label={t("settings.chooseKeyFile")} />
                          </Space.Compact>
                          {tlsStatusHint}
                        </ValidatedField>
                      </SettingRow>
                      <SettingRow title={t("settings.generateCertificate")}>
                        <Button onClick={openCertificateModal}>{t("settings.generateCertificate")}</Button>
                      </SettingRow>
                    </>
                  )}
                </>
              ) : null}
            </div>
          ) : null}

          {section === "system-dns" ? (
            <SystemDnsPage
              embedded
              systemDns={systemDns}
              loading={systemDnsLoading || busy}
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
                <SettingRow title={t("settings.maxEntries")} description={t("settings.maxEntriesDescription")}>
                  <InlineNumberSetting value={cfg.cache.maxEntries} error={validation.cache.maxEntries} onChange={(value) => updateCache("maxEntries", value)} />
                </SettingRow>
                <SettingRow title={t("settings.maxEntrySize")} description={t("settings.maxEntrySizeDescription")}>
                  <InlineNumberSetting value={cfg.cache.maxEntrySize} error={validation.cache.maxEntrySize} onChange={(value) => updateCache("maxEntrySize", value)} />
                </SettingRow>
                <SettingRow title={t("settings.minTtl")} description={t("settings.minTtlDescription")}>
                  <InlineNumberSetting value={cfg.cache.minTTL} error={validation.cache.minTTL} onChange={(value) => updateCache("minTTL", value)} />
                </SettingRow>
                <SettingRow title={t("settings.maxTtl")} description={t("settings.maxTtlDescription")}>
                  <InlineNumberSetting value={cfg.cache.maxTTL} error={validation.cache.maxTTL} onChange={(value) => updateCache("maxTTL", value)} />
                </SettingRow>
                <SettingRow title={t("settings.negativeTtl")} description={t("settings.negativeTtlDescription")}>
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
                <ValidatedField error={validation.healthcheck.interval}>
                  <CommitOnBlurInput status={validation.healthcheck.interval ? "error" : undefined} value={cfg.healthcheck.interval} onCommit={(value) => updateHealthcheck("interval", value)} placeholder="30s" />
                </ValidatedField>
              </SettingRow>
              <SettingRow title={t("settings.healthTimeout")}>
                <ValidatedField error={validation.healthcheck.timeout}>
                  <CommitOnBlurInput status={validation.healthcheck.timeout ? "error" : undefined} value={cfg.healthcheck.timeout} onCommit={(value) => updateHealthcheck("timeout", value)} placeholder="2s" />
                </ValidatedField>
              </SettingRow>
              <SettingRow title={t("settings.healthDomain")}>
                <ValidatedField error={validation.healthcheck.domain}>
                  <CommitOnBlurInput status={validation.healthcheck.domain ? "error" : undefined} value={cfg.healthcheck.domain} onCommit={(value) => updateHealthcheck("domain", value)} placeholder="." />
                </ValidatedField>
              </SettingRow>
              <SettingRow title={t("settings.failureThreshold")} description={t("settings.failureThresholdDescription")}>
                <InlineNumberSetting value={cfg.healthcheck.failureThreshold} error={validation.healthcheck.failureThreshold} onChange={(value) => updateHealthcheck("failureThreshold", value)} />
              </SettingRow>
              <SettingRow title={t("settings.recoveryThreshold")} description={t("settings.recoveryThresholdDescription")}>
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
  const [draftValue, setDraftValue] = useState<number | string>(value);

  useEffect(() => {
    if (draftValue !== "") {
      setDraftValue(value);
    }
  }, [value]);

  return (
    <ValidatedField error={error}>
      <InputNumber
        className="workbenchInlineNumber"
        status={error ? "error" : undefined}
        min={0}
        value={draftValue}
        placeholder={t("settings.defaultPlaceholder")}
        onChange={(next) => {
          if (next === null || next === undefined || next === "") {
            setDraftValue("");
            return;
          }
          setDraftValue(next);
        }}
        onBlur={() => {
          if (draftValue === "") {
            setDraftValue(value);
            return;
          }
          if (typeof draftValue === "number" && Number.isFinite(draftValue) && draftValue !== value) {
            onChange(draftValue);
          }
        }}
        onPressEnter={(event) => event.currentTarget.blur()}
      />
    </ValidatedField>
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
        <strong>
          {title}
          {required ? <span className="settingRequiredMark">*</span> : null}
          <HintTooltip hint={description} />
        </strong>
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


