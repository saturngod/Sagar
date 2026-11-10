/** AI provider settings DTOs from the Rust settings service. */

export type CliProviderStatus = {
  id: string;
  label: string;
  binaries: string[];
  available: boolean;
};

export type CliCheckResult = {
  id: string;
  label: string;
  available: boolean;
  connected: boolean;
  version: string | null;
  message: string;
};

export type ApiProtocol = "openai" | "gemini" | "anthropic";

export type CustomProviderSummary = {
  protocol: ApiProtocol;
  id: string;
  name: string;
  baseUrl: string;
  model: string;
  hasApiKey: boolean;
  createdAt: string;
  updatedAt: string;
};

export type AiProviderList = {
  cli: CliProviderStatus[];
  custom: CustomProviderSummary[];
};

export type CustomTestResult = {
  ok: boolean;
  modelCount: number | null;
  message: string;
};

export type TranslationModel = {
  id: string;
  label: string;
  detail: string | null;
  reasoningEfforts: string[] | null;
};

export type TranslationModelList = {
  agent: string;
  source: "live" | "cached" | "catalog";
  models: TranslationModel[];
  message: string;
};

export type TranslationSelection = {
  reasoningEffort: string | null;
  agent: string;
  model: string;
  updatedAt: string;
};

export type AiTranslationStart = {
  jobId: string;
};

export type AiTranslationEvent = {
  jobId: string;
  chapterId: string;
  kind: "started" | "status" | "stream" | "done" | "failed" | "cancelled";
  proposalHtml: string | null;
  /** Running translated text for `stream` events. */
  partialText: string | null;
  message: string;
};

export type AiHtmlFixStart = {
  jobId: string;
};

export type AiHtmlFixEvent = {
  jobId: string;
  chapterId: string;
  kind: "started" | "status" | "stream" | "done" | "failed" | "cancelled";
  fixedHtml: string | null;
  /** Running translated text for `stream` events. */
  partialText: string | null;
  message: string;
};
