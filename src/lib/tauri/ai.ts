import { invoke } from "@tauri-apps/api/core";
import type {
  AiHtmlFixStart,
  AiProviderList,
  AiTranslationStart,
  ApiProtocol,
  CliCheckResult,
  CustomProviderSummary,
  CustomTestResult,
  TranslationModelList,
  TranslationSelection,
} from "../../types/ai";
import { toCommandError } from "./errors";

/** Typed wrappers for the AI provider settings commands (settings only). */

export async function listAiProviders(): Promise<AiProviderList> {
  try {
    return await invoke<AiProviderList>("list_ai_providers");
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function checkAiCli(providerId: string): Promise<CliCheckResult> {
  try {
    return await invoke<CliCheckResult>("check_ai_cli", { providerId });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function saveAiCustomProvider(input: {
  protocol: ApiProtocol;
  id?: string;
  name: string;
  baseUrl: string;
  apiKey?: string | null;
  model: string;
}): Promise<CustomProviderSummary> {
  try {
    return await invoke<CustomProviderSummary>("save_ai_custom_provider", {
      protocol: input.protocol,
      id: input.id ?? null,
      name: input.name,
      baseUrl: input.baseUrl,
      apiKey: input.apiKey ?? null,
      model: input.model,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function deleteAiCustomProvider(id: string): Promise<void> {
  try {
    await invoke("delete_ai_custom_provider", { id });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function testAiCustomProvider(id: string): Promise<CustomTestResult> {
  try {
    return await invoke<CustomTestResult>("test_ai_custom_provider", { id });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function listTranslationModels(
  agent: string,
  refresh = false,
): Promise<TranslationModelList> {
  try {
    return await invoke<TranslationModelList>("list_translation_models", {
      agent,
      refresh,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function getTranslationSelection(): Promise<TranslationSelection | null> {
  try {
    return await invoke<TranslationSelection | null>("get_translation_selection");
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function setTranslationSelection(
  agent: string,
  model: string,
  reasoningEffort: string | null,
): Promise<TranslationSelection> {
  try {
    return await invoke<TranslationSelection>("set_translation_selection", {
      agent,
      model,
      reasoningEffort,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function startAiTranslation(input: {
  chapterId: string;
  agent: string;
  model: string;
  consent: boolean;
  previousOutput?: string;
  currentInstructions?: string;
  reasoningEffort?: string | null;
}): Promise<AiTranslationStart> {
  try {
    return await invoke<AiTranslationStart>("start_ai_translation", {
      chapterId: input.chapterId,
      agent: input.agent,
      model: input.model,
      consent: input.consent,
      previousOutput: input.previousOutput ?? null,
      currentInstructions: input.currentInstructions ?? null,
      reasoningEffort: input.reasoningEffort ?? null,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}

export async function cancelAiTranslation(jobId: string): Promise<void> {
  try {
    await invoke("cancel_ai_translation", { jobId });
  } catch (error) {
    throw toCommandError(error);
  }
}

/** Starts an HTML-fix run over the user's current draft HTML. */
export async function startAiHtmlFix(input: {
  chapterId: string;
  html: string;
  agent: string;
  model: string;
  consent: boolean;
  reasoningEffort?: string | null;
}): Promise<AiHtmlFixStart> {
  try {
    return await invoke<AiHtmlFixStart>("start_ai_html_fix", {
      chapterId: input.chapterId,
      html: input.html,
      agent: input.agent,
      model: input.model,
      consent: input.consent,
      reasoningEffort: input.reasoningEffort ?? null,
    });
  } catch (error) {
    throw toCommandError(error);
  }
}
