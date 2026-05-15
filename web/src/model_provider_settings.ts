import {
  deleteModelProfile,
  discoverModelProfile,
  loadModelCatalog,
  loadModelFallbackConfig,
  loadModelProfiles,
  probeModelProfile,
  refreshModelCatalog,
  saveModelProfile
} from "./api/client.js";
import type {
  ModelCatalogProvider,
  ModelCatalogResult,
  ModelFallbackConfig,
  ModelProfileSaveRequest,
  ModelProfilesResponse,
  ModelProfileView,
  ModelProviderKind
} from "./api/contracts";
import { element, icon, statusPill, textElement } from "./ui.js";

type ProviderUiState = {
  loading: boolean;
  profiles: ModelProfilesResponse | null;
  fallback: ModelFallbackConfig | null;
  catalog: ModelCatalogResult | null;
  loadFailed: boolean;
  selectedProfile: string;
  draft: ModelProfileDraft;
  message: string;
  result: unknown;
};

type ModelProfileDraft = {
  name: string;
  provider: ModelProviderKind;
  model: string;
  baseUrl: string;
  apiKey: string;
  clearApiKey: boolean;
  temperature: number;
  topP: number;
  maxTokens: number;
  contextWindow: number;
  connectTimeoutSeconds: number;
  fallbackPolicyId: string;
  fallbackPriority: number;
  isDefault: boolean;
};

type ProviderCallbacks = {
  rerender: () => void;
  errorMessage: (error: unknown) => string;
};

const PROVIDERS: Array<[ModelProviderKind, string]> = [
  ["openai_compatible", "OpenAI-compatible"],
  ["anthropic", "Anthropic"],
  ["bigmodel", "BigModel"],
  ["minimax", "MiniMax"],
  ["maas", "MaaS"],
  ["codeagent", "CodeAgent"],
  ["echo", "Echo"]
];

let providerState: ProviderUiState = {
  loading: false,
  profiles: null,
  fallback: null,
  catalog: null,
  loadFailed: false,
  selectedProfile: "",
  draft: emptyDraft(),
  message: "",
  result: null
};

export function modelProviderSettingsPanel(callbacks: ProviderCallbacks): HTMLElement {
  ensureProviderData(callbacks);
  const panel = element("div", "settings-group model-provider-settings");
  panel.append(textElement("div", "panel-title", "Model providers"));
  panel.append(providerStatusRow());
  const layout = element("div", "model-provider-layout");
  layout.append(profileList(callbacks), profileEditor(callbacks));
  panel.append(layout, providerResultPanel());

  return panel;
}

function ensureProviderData(callbacks: ProviderCallbacks) {
  if (
    providerState.loading ||
    providerState.loadFailed ||
    (providerState.profiles && providerState.fallback && providerState.catalog)
  ) {
    return;
  }
  providerState.loading = true;
  void Promise.all([loadModelProfiles(), loadModelFallbackConfig(), loadModelCatalog()])
    .then(([profiles, fallback, catalog]) => {
      providerState = {
        ...providerState,
        loading: false,
        profiles,
        fallback,
        catalog,
        loadFailed: false,
        selectedProfile: profiles.default_profile ?? profiles.profiles[0]?.name ?? "",
        draft: draftFromProfile(profiles.profiles.find((profile) => profile.is_default) ?? profiles.profiles[0])
      };
      callbacks.rerender();
    })
    .catch((error) => {
      providerState = {
        ...providerState,
        loading: false,
        loadFailed: true,
        message: callbacks.errorMessage(error)
      };
      callbacks.rerender();
    });
}

function providerStatusRow(): HTMLElement {
  const row = element("div", "settings-status-row");
  const profiles = providerState.profiles;
  row.append(
    statusPill(`${profiles?.profiles.length ?? 0} profiles`, profiles?.profiles.length ? "good" : "warn"),
    statusPill(
      profiles?.default_profile ? `default ${profiles.default_profile}` : "no default",
      profiles?.default_profile ? "good" : "warn"
    ),
    statusPill(
      providerState.catalog?.ok ? "catalog ready" : "catalog fallback",
      providerState.catalog?.ok ? "good" : "warn"
    )
  );

  return row;
}

function profileList(callbacks: ProviderCallbacks): HTMLElement {
  const list = element("div", "model-profile-list");
  const add = document.createElement("button");
  add.type = "button";
  add.className = "button";
  add.append(icon("add-icon"), document.createTextNode("New"));
  add.addEventListener("click", () => {
    providerState.selectedProfile = "";
    providerState.draft = emptyDraft();
    providerState.message = "";
    providerState.result = null;
    callbacks.rerender();
  });
  list.append(add);

  for (const profile of providerState.profiles?.profiles ?? []) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = profile.name === providerState.selectedProfile ? "model-profile-row active" : "model-profile-row";
    button.append(
      textElement("span", "model-profile-name", profile.name),
      textElement("span", "model-profile-meta", `${providerLabel(profile.provider)} / ${profile.model}`)
    );
    button.addEventListener("click", () => {
      providerState.selectedProfile = profile.name;
      providerState.draft = draftFromProfile(profile);
      providerState.message = "";
      providerState.result = null;
      callbacks.rerender();
    });
    list.append(button);
  }

  return list;
}

function profileEditor(callbacks: ProviderCallbacks): HTMLElement {
  const form = element("form", "model-profile-editor");
  form.addEventListener("submit", (event) => event.preventDefault());
  const draft = providerState.draft;
  form.append(
    textControl("Profile name", draft.name, (value) => {
      draft.name = value;
    }),
    selectControl("Provider", draft.provider, PROVIDERS, (value) => {
      draft.provider = value as ModelProviderKind;
      const baseUrl = defaultBaseUrl(draft.provider);
      if (!draft.baseUrl && baseUrl) {
        draft.baseUrl = baseUrl;
      }
      callbacks.rerender();
    }),
    catalogPicker(callbacks),
    textControl("Model", draft.model, (value) => {
      draft.model = value;
    }),
    textControl("Base URL", draft.baseUrl, (value) => {
      draft.baseUrl = value;
    }),
    passwordControl("API key", draft.apiKey, (value) => {
      draft.apiKey = value;
      if (value.trim()) {
        draft.clearApiKey = false;
      }
    }),
    checkboxControl("Clear API key", draft.clearApiKey, (checked) => {
      draft.clearApiKey = checked;
      if (checked) {
        draft.apiKey = "";
      }
    }),
    numberControl("Temperature", draft.temperature, "0", "2", "0.1", (value) => {
      draft.temperature = numericValue(value, 0.7);
    }),
    numberControl("Top P", draft.topP, "0", "1", "0.05", (value) => {
      draft.topP = numericValue(value, 1);
    }),
    numberControl("Max tokens", draft.maxTokens, "0", "200000", "1", (value) => {
      draft.maxTokens = numericValue(value, 0);
    }),
    numberControl("Context window", draft.contextWindow, "0", "2000000", "1", (value) => {
      draft.contextWindow = numericValue(value, 0);
    }),
    numberControl("Connect timeout", draft.connectTimeoutSeconds, "1", "300", "1", (value) => {
      draft.connectTimeoutSeconds = numericValue(value, 30);
    }),
    fallbackSelect(draft),
    checkboxControl("Default profile", draft.isDefault, (checked) => {
      draft.isDefault = checked;
    }),
    profileActions(callbacks)
  );

  return form;
}

function catalogPicker(callbacks: ProviderCallbacks): HTMLElement {
  const group = element("div", "model-catalog-picker");
  const providers = providerState.catalog?.providers ?? [];
  const providerSelect = document.createElement("select");
  providerSelect.name = "catalog-provider";
  providerSelect.append(option("", "Catalog provider"));
  for (const provider of providers) {
    providerSelect.append(option(provider.id, provider.name));
  }
  providerSelect.addEventListener("change", () => {
    const provider = providers.find((item) => item.id === providerSelect.value);
    if (!provider) {
      return;
    }
    providerState.draft.provider = provider.runtime_provider;
    providerState.draft.baseUrl = defaultBaseUrl(provider.runtime_provider) ?? providerState.draft.baseUrl;
    callbacks.rerender();
  });

  const modelSelect = document.createElement("select");
  modelSelect.name = "catalog-model";
  modelSelect.append(option("", "Catalog model"));
  for (const provider of providers) {
    for (const model of provider.models) {
      const item = option(`${provider.id}:${model.id}`, `${provider.name} / ${model.name}`);
      modelSelect.append(item);
    }
  }
  modelSelect.addEventListener("change", () => {
    const [providerId, modelId] = modelSelect.value.split(":");
    const provider = providers.find((item) => item.id === providerId);
    const model = provider?.models.find((item) => item.id === modelId);
    if (!provider || !model) {
      return;
    }
    providerState.draft.provider = provider.runtime_provider;
    providerState.draft.model = model.id;
    providerState.draft.contextWindow = model.context_window ?? providerState.draft.contextWindow;
    callbacks.rerender();
  });

  const refresh = document.createElement("button");
  refresh.type = "button";
  refresh.className = "button";
  refresh.append(icon("refresh-icon"), document.createTextNode("Refresh catalog"));
  refresh.addEventListener("click", () => {
    providerState.message = "Refreshing catalog";
    callbacks.rerender();
    void refreshModelCatalog()
      .then((catalog) => {
        providerState.catalog = catalog;
        providerState.message = catalog.ok ? "Catalog refreshed" : catalog.error_message ?? "Catalog refresh failed";
        callbacks.rerender();
      })
      .catch((error) => {
        providerState.message = callbacks.errorMessage(error);
        callbacks.rerender();
      });
  });

  group.append(providerSelect, modelSelect, refresh);
  return group;
}

function fallbackSelect(draft: ModelProfileDraft): HTMLElement {
  const options = [["", "Fallback policy"] as [string, string]].concat(
    (providerState.fallback?.policies ?? []).map((policy) => [policy.policy_id, policy.name])
  );

  return selectControl("Fallback policy", draft.fallbackPolicyId, options, (value) => {
    draft.fallbackPolicyId = value;
  });
}

function profileActions(callbacks: ProviderCallbacks): HTMLElement {
  const actions = element("div", "settings-actions");
  const save = document.createElement("button");
  save.type = "button";
  save.className = "button primary";
  save.dataset.testid = "save-model-profile";
  save.append(icon("save-icon"), document.createTextNode("Save profile"));
  save.addEventListener("click", () => {
    void saveDraft(callbacks);
  });

  const probe = document.createElement("button");
  probe.type = "button";
  probe.className = "button";
  probe.dataset.testid = "probe-model-profile";
  probe.append(icon("run-icon"), document.createTextNode("Probe"));
  probe.addEventListener("click", () => {
    void runProbe(callbacks);
  });

  const discover = document.createElement("button");
  discover.type = "button";
  discover.className = "button";
  discover.dataset.testid = "discover-model-profile";
  discover.append(icon("search-icon"), document.createTextNode("Discover"));
  discover.addEventListener("click", () => {
    void runDiscover(callbacks);
  });

  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "button";
  remove.dataset.testid = "delete-model-profile";
  remove.disabled = !providerState.selectedProfile;
  remove.append(icon("trash-icon"), document.createTextNode("Delete"));
  remove.addEventListener("click", () => {
    void deleteDraft(callbacks);
  });

  actions.append(save, probe, discover, remove);
  return actions;
}

async function saveDraft(callbacks: ProviderCallbacks) {
  try {
    const draft = providerState.draft;
    const name = draft.name.trim();
    const profiles = await saveModelProfile(name, draftPayload(draft));
    providerState.profiles = profiles;
    providerState.selectedProfile = name;
    providerState.draft = draftFromProfile(profiles.profiles.find((profile) => profile.name === name));
    providerState.message = "Profile saved";
    callbacks.rerender();
  } catch (error) {
    providerState.message = callbacks.errorMessage(error);
    callbacks.rerender();
  }
}

async function deleteDraft(callbacks: ProviderCallbacks) {
  if (!providerState.selectedProfile) {
    return;
  }
  try {
    const profiles = await deleteModelProfile(providerState.selectedProfile);
    providerState.profiles = profiles;
    providerState.selectedProfile = profiles.default_profile ?? profiles.profiles[0]?.name ?? "";
    providerState.draft = draftFromProfile(profiles.profiles.find((profile) => profile.name === providerState.selectedProfile));
    providerState.message = "Profile deleted";
    callbacks.rerender();
  } catch (error) {
    providerState.message = callbacks.errorMessage(error);
    callbacks.rerender();
  }
}

async function runProbe(callbacks: ProviderCallbacks) {
  providerState.message = "Probing provider";
  callbacks.rerender();
  try {
    providerState.result = await probeModelProfile(probeRequest());
    providerState.message = "Probe complete";
  } catch (error) {
    providerState.message = callbacks.errorMessage(error);
  }
  callbacks.rerender();
}

async function runDiscover(callbacks: ProviderCallbacks) {
  providerState.message = "Discovering models";
  callbacks.rerender();
  try {
    providerState.result = await discoverModelProfile(probeRequest());
    providerState.message = "Discovery complete";
  } catch (error) {
    providerState.message = callbacks.errorMessage(error);
  }
  callbacks.rerender();
}

function probeRequest(): {
  profile_name?: string;
  override_config: ModelProfileSaveRequest;
} {
  return {
    profile_name: providerState.selectedProfile || undefined,
    override_config: draftPayload(providerState.draft)
  };
}

function providerResultPanel(): HTMLElement {
  const panel = element("div", "model-provider-result");
  if (providerState.message) {
    panel.append(textElement("div", "muted-line", providerState.message));
  }
  if (providerState.result) {
    const pre = document.createElement("pre");
    pre.className = "settings-model-provider-preview";
    pre.textContent = JSON.stringify(providerState.result, null, 2);
    panel.append(pre);
  }

  return panel;
}

function draftPayload(draft: ModelProfileDraft): ModelProfileSaveRequest {
  return {
    provider: draft.provider,
    model: draft.model.trim(),
    base_url: draft.baseUrl.trim() || undefined,
    api_key: draft.apiKey.trim() || undefined,
    temperature: draft.temperature,
    top_p: draft.topP,
    max_tokens: draft.maxTokens || undefined,
    context_window: draft.contextWindow || undefined,
    connect_timeout_seconds: draft.connectTimeoutSeconds,
    fallback_policy_id: draft.fallbackPolicyId || undefined,
    fallback_priority: draft.fallbackPriority,
    clear_api_key: draft.clearApiKey || undefined,
    is_default: draft.isDefault
  };
}

function draftFromProfile(profile: ModelProfileView | undefined): ModelProfileDraft {
  if (!profile) {
    return emptyDraft();
  }

  return {
    name: profile.name,
    provider: profile.provider,
    model: profile.model,
    baseUrl: profile.base_url,
    apiKey: "",
    clearApiKey: false,
    temperature: profile.temperature,
    topP: profile.top_p,
    maxTokens: profile.max_tokens ?? 0,
    contextWindow: profile.context_window ?? 0,
    connectTimeoutSeconds: profile.connect_timeout_seconds,
    fallbackPolicyId: profile.fallback_policy_id ?? "",
    fallbackPriority: profile.fallback_priority,
    isDefault: profile.is_default
  };
}

function emptyDraft(): ModelProfileDraft {
  return {
    name: "default",
    provider: "openai_compatible",
    model: "",
    baseUrl: "",
    apiKey: "",
    clearApiKey: false,
    temperature: 0.7,
    topP: 1,
    maxTokens: 0,
    contextWindow: 0,
    connectTimeoutSeconds: 30,
    fallbackPolicyId: "",
    fallbackPriority: 0,
    isDefault: true
  };
}

function providerLabel(provider: ModelProviderKind): string {
  return PROVIDERS.find(([value]) => value === provider)?.[1] ?? provider;
}

function defaultBaseUrl(provider: ModelProviderKind): string | null {
  if (provider === "anthropic") {
    return "https://api.anthropic.com";
  }
  if (provider === "codeagent") {
    return "https://codeagentcli.rnd.huawei.com/codeAgentPro";
  }
  if (provider === "maas") {
    return "http://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2/";
  }
  if (provider === "echo") {
    return "http://127.0.0.1/echo";
  }

  return null;
}

function option(value: string, label: string): HTMLOptionElement {
  const item = document.createElement("option");
  item.value = value;
  item.textContent = label;
  return item;
}

function textControl(label: string, value: string, onInput: (value: string) => void): HTMLElement {
  const control = fieldShell(label);
  const input = document.createElement("input");
  input.name = fieldName(label);
  input.setAttribute("aria-label", label);
  input.value = value;
  input.addEventListener("input", () => onInput(input.value));
  control.append(input);
  return control;
}

function passwordControl(label: string, value: string, onInput: (value: string) => void): HTMLElement {
  const control = fieldShell(label);
  const input = document.createElement("input");
  input.type = "password";
  input.name = fieldName(label);
  input.setAttribute("aria-label", label);
  input.autocomplete = "new-password";
  input.placeholder = "leave blank to keep current key";
  input.value = value;
  input.addEventListener("input", () => onInput(input.value));
  control.append(input);
  return control;
}

function numberControl(
  label: string,
  value: number,
  min: string,
  max: string,
  step: string,
  onInput: (value: string) => void
): HTMLElement {
  const control = fieldShell(label);
  const input = document.createElement("input");
  input.type = "number";
  input.name = fieldName(label);
  input.setAttribute("aria-label", label);
  input.min = min;
  input.max = max;
  input.step = step;
  input.value = String(value);
  input.addEventListener("input", () => onInput(input.value));
  control.append(input);
  return control;
}

function selectControl<T extends string>(
  label: string,
  value: T,
  options: Array<[T, string]>,
  onChange: (value: string) => void
): HTMLElement {
  const control = fieldShell(label);
  const select = document.createElement("select");
  select.name = fieldName(label);
  select.setAttribute("aria-label", label);
  for (const [optionValue, optionLabel] of options) {
    const item = option(optionValue, optionLabel);
    item.selected = optionValue === value;
    select.append(item);
  }
  select.addEventListener("change", () => onChange(select.value));
  control.append(select);
  return control;
}

function checkboxControl(
  label: string,
  checked: boolean,
  onChange: (checked: boolean) => void
): HTMLElement {
  const control = element("label", "checkbox-row settings-checkbox");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.name = fieldName(label);
  input.setAttribute("aria-label", label);
  input.checked = checked;
  input.addEventListener("change", () => onChange(input.checked));
  control.append(input, textElement("span", undefined, label));
  return control;
}

function fieldShell(label: string): HTMLElement {
  const control = element("label", "field");
  control.append(textElement("span", undefined, label));
  return control;
}

function fieldName(label: string): string {
  return label.toLowerCase().replaceAll(" ", "-");
}

function numericValue(value: string, fallback: number): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
}
