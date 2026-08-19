import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";

interface Component {
  name: string;
  source_url: string;
  component_ref: string;
  default: boolean;
  binds_to_project_type?: string | null;
}

interface ComponentResult {
  name: string;
  outcome: string;
  message: string | null;
}

interface Manifest {
  manifest_version: string;
  components: Component[];
  gui: {
    theme: "system" | "light" | "dark";
    app_name: string | null;
  };
}

interface OptionChoice {
  value: string;
  label: string;
  recommended?: boolean;
}

interface OptionSchema {
  key: string;
  label: string;
  type: "choice" | "bool" | string;
  choices?: OptionChoice[];
  default?: string | boolean | null;
}

interface OptionsResponse {
  schema_version: number;
  options: OptionSchema[];
}

interface InstallDone {
  success: boolean;
  message: string;
}

interface InstalledComponentState {
  version: string;
  state: string;
}

interface InstalledStatus {
  manifest_version: string;
  components: Record<string, InstalledComponentState>;
}

let logView: HTMLElement | null;
let installButton: HTMLButtonElement | null;
let statusEl: HTMLElement | null;
let currentManifest: Manifest | null = null;
let selectedProjectPath: string | null = null;

const selectedOptionValues: Record<string, Record<string, string>> = {};

function appendLog(line: string) {
  if (!logView) return;
  logView.textContent += line + "\n";
  logView.scrollTop = logView.scrollHeight;
}

function currentInstallRoot(): string {
  return (document.querySelector<HTMLInputElement>("#install-root")?.value ?? "").trim();
}

function renderComponents(manifest: Manifest) {
  const container = document.querySelector<HTMLDivElement>("#components-list");
  const versionEl = document.querySelector<HTMLElement>("#manifest-version");
  if (versionEl) versionEl.textContent = `Manifest ${manifest.manifest_version}`;
  if (!container) return;
  container.innerHTML = "";
  for (const c of manifest.components) {
    const label = document.createElement("label");
    label.className = "component-row";

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.value = c.name;
    checkbox.checked = c.default;
    checkbox.dataset.componentName = c.name;
    checkbox.addEventListener("change", refreshModelOptions);

    const text = document.createElement("div");
    const title = document.createElement("div");
    title.className = "component-title";
    title.textContent = `${c.name} (${c.component_ref})`;
    text.appendChild(title);

    label.appendChild(checkbox);
    label.appendChild(text);
    container.appendChild(label);
  }
}

function selectedComponents(): string[] {
  const boxes = document.querySelectorAll<HTMLInputElement>(
    "#components-list input[type=checkbox]:checked",
  );
  return Array.from(boxes).map((b) => b.value);
}

async function loadDefaultInstallRoot() {
  const input = document.querySelector<HTMLInputElement>("#install-root");
  if (!input || input.value.trim()) return;
  try {
    input.value = await invoke<string>("default_install_root");
  } catch {
    // Leave blank; run_install resolves the same default server-side.
  }
}

async function loadComponents() {
  try {
    const manifest = await invoke<Manifest>("list_components");
    currentManifest = manifest;
    if (manifest.gui.theme !== "system") {
      document.documentElement.dataset.theme = manifest.gui.theme;
    }
    if (manifest.gui.app_name) {
      try {
        await getCurrentWindow().setTitle(manifest.gui.app_name);
      } catch (e) {
        console.error(`Could not set window title to "${manifest.gui.app_name}":`, e);
      }
    }
    renderComponents(manifest);
    await refreshModelOptions();
  } catch (e) {
    const container = document.querySelector<HTMLDivElement>("#components-list");
    if (container) container.textContent = `Could not load manifest.toml: ${e}`;
  }
}

function renderOptionControl(componentName: string, schema: OptionSchema): HTMLElement {
  const row = document.createElement("div");
  row.className = "model-option-row";
  const labelEl = document.createElement("div");
  labelEl.className = "model-option-label";
  labelEl.textContent = `${componentName}: ${schema.label}`;
  row.appendChild(labelEl);

  if (schema.type === "choice" && schema.choices) {
    const select = document.createElement("select");
    for (const choice of schema.choices) {
      const opt = document.createElement("option");
      opt.value = choice.value;
      opt.textContent = choice.recommended ? `${choice.label} (recommended)` : choice.label;
      select.appendChild(opt);
    }
    const defaultValue = typeof schema.default === "string" ? schema.default : undefined;
    if (defaultValue) select.value = defaultValue;
    selectedOptionValues[componentName] ??= {};
    selectedOptionValues[componentName][schema.key] = select.value;
    select.addEventListener("change", () => {
      selectedOptionValues[componentName][schema.key] = select.value;
    });
    row.appendChild(select);
  } else if (schema.type === "bool") {
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = schema.default === true;
    selectedOptionValues[componentName] ??= {};
    selectedOptionValues[componentName][schema.key] = String(checkbox.checked);
    checkbox.addEventListener("change", () => {
      selectedOptionValues[componentName][schema.key] = String(checkbox.checked);
    });
    row.appendChild(checkbox);
  } else {
    const note = document.createElement("span");
    note.className = "muted";
    note.textContent = `(unsupported option type "${schema.type}")`;
    row.appendChild(note);
  }
  return row;
}

async function refreshModelOptions() {
  const section = document.querySelector<HTMLElement>("#model-options-section");
  const container = document.querySelector<HTMLDivElement>("#model-options-list");
  if (!section || !container) return;

  container.innerHTML = "";
  const installRoot = currentInstallRoot();
  const selected = selectedComponents();
  let anyShown = false;

  for (const name of selected) {
    let response: OptionsResponse | null;
    try {
      response = await invoke<OptionsResponse | null>("describe_component_options", {
        component: name,
        installRoot: installRoot || null,
      });
    } catch {
      response = null;
    }
    if (!response || response.options.length === 0) continue;
    anyShown = true;
    for (const schema of response.options) {
      container.appendChild(renderOptionControl(name, schema));
    }
  }

  section.classList.toggle("hidden", !anyShown);
}

function populateProjectTypeDropdown(installedStatus: InstalledStatus) {
  const section = document.querySelector<HTMLElement>("#add-project-section");
  const select = document.querySelector<HTMLSelectElement>("#project-type-select");
  if (!section || !select || !currentManifest) return;

  const installedNames = new Set(Object.keys(installedStatus.components ?? {}));
  const types = new Set(
    currentManifest.components
      .filter((c) => installedNames.has(c.name) && c.binds_to_project_type)
      .map((c) => c.binds_to_project_type as string),
  );

  select.innerHTML = "";
  for (const t of types) {
    const option = document.createElement("option");
    option.value = t;
    option.textContent = t;
    select.appendChild(option);
  }
  section.classList.toggle("hidden", types.size === 0);
}

async function refreshInstallStatus() {
  const statusSpan = document.querySelector<HTMLElement>("#install-status");
  try {
    const status = await invoke<InstalledStatus>("read_install_status", {
      installRoot: currentInstallRoot() || null,
    });
    populateProjectTypeDropdown(status);
    if (statusSpan) {
      const entries = Object.entries(status.components ?? {});
      statusSpan.textContent =
        entries.length === 0
          ? ""
          : `Already installed here - ${entries.map(([n, c]) => `${n}: ${c.state} (${c.version})`).join(", ")}`;
    }
  } catch {
    if (statusSpan) statusSpan.textContent = "";
  }
  await refreshModelOptions();
}

const MODE_BUTTON_LABELS: Record<string, string> = {
  install: "Install",
  force: "Force Reinstall",
  repair: "Repair",
};

const MODE_RUNNING_LABELS: Record<string, string> = {
  install: "Installing…",
  force: "Reinstalling…",
  repair: "Repairing…",
};

function currentMode(): string {
  return document.querySelector<HTMLSelectElement>("#mode-select")?.value ?? "install";
}

function updateInstallButtonLabel() {
  if (installButton) installButton.textContent = MODE_BUTTON_LABELS[currentMode()] ?? "Install";
}

async function runInstall() {
  if (!installButton || !statusEl) return;
  const components = selectedComponents();
  if (components.length === 0) {
    statusEl.textContent = "Pick at least one component first.";
    return;
  }
  const installRoot = currentInstallRoot();
  const mode = currentMode();

  const options: Record<string, Record<string, string>> = {};
  for (const name of components) {
    if (selectedOptionValues[name]) options[name] = selectedOptionValues[name];
  }

  installButton.disabled = true;
  statusEl.textContent = MODE_RUNNING_LABELS[mode] ?? "Installing…";
  statusEl.className = "status";
  if (logView) logView.textContent = "";

  try {
    await invoke("run_install", {
      components,
      installRoot: installRoot || null,
      mode,
      options,
    });
  } catch (e) {
    statusEl.textContent = `Failed to start: ${e}`;
    installButton.disabled = false;
  }
}

async function pickProjectFile() {
  const path = await open({ multiple: false, directory: false });
  if (typeof path === "string") {
    selectedProjectPath = path;
    const span = document.querySelector<HTMLSpanElement>("#selected-project-path");
    if (span) span.textContent = path;
    const bindButton = document.querySelector<HTMLButtonElement>("#bind-project-button");
    if (bindButton) bindButton.disabled = false;
  }
}

async function bindProject() {
  if (!selectedProjectPath) return;
  const projectType =
    document.querySelector<HTMLSelectElement>("#project-type-select")?.value ?? "";
  const resultDiv = document.querySelector<HTMLDivElement>("#bind-project-result");
  if (!resultDiv) return;
  try {
    const results = await invoke<ComponentResult[]>("bind_project", {
      installRoot: currentInstallRoot() || null,
      projectType,
      projectPath: selectedProjectPath,
    });
    resultDiv.textContent = `Bound ${results.length} component(s) to ${projectType}.`;
  } catch (err) {
    resultDiv.textContent = `Failed to bind project: ${err}`;
  }
}

window.addEventListener("DOMContentLoaded", () => {
  logView = document.querySelector("#log-view");
  installButton = document.querySelector("#install-button");
  statusEl = document.querySelector("#status");

  installButton?.addEventListener("click", runInstall);

  const modeSelect = document.querySelector<HTMLSelectElement>("#mode-select");
  modeSelect?.addEventListener("change", updateInstallButtonLabel);
  updateInstallButtonLabel();

  const installRootInput = document.querySelector<HTMLInputElement>("#install-root");
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  installRootInput?.addEventListener("input", () => {
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(refreshInstallStatus, 400);
  });

  document
    .querySelector<HTMLButtonElement>("#pick-project-file-button")
    ?.addEventListener("click", pickProjectFile);
  document
    .querySelector<HTMLButtonElement>("#bind-project-button")
    ?.addEventListener("click", bindProject);

  listen<string>("install-log", (event) => appendLog(event.payload));
  listen<InstallDone>("install-done", (event) => {
    if (statusEl) {
      statusEl.textContent = event.payload.message;
      statusEl.className = "status " + (event.payload.success ? "status-ok" : "status-fail");
    }
    if (installButton) installButton.disabled = false;
    if (event.payload.success) refreshInstallStatus();
  });

  loadDefaultInstallRoot().then(() => loadComponents().then(refreshInstallStatus));
});
