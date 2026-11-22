<script setup>
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

defineProps({
  folderPath: {
    type: String,
    required: true,
  },
  os: {
    type: String,
    required: true,
  },
});

const emit = defineEmits(["back"]);
const menuOpen = ref(false);
const versionInfo = ref({
  installedVersion: "...",
  latestVersion: null,
  isNewest: false,
  showUpdate: false,
});
const isUpdating = ref(false);
const downloadPercent = ref(null);
const updateError = ref("");
const isRunning = ref(false);
const allFilesGenerated = ref(true);
const isGenerating = ref(false);
const serverError = ref("");
const worldSettings = ref([]);
const serverSettings = ref([]);
const settingsError = ref("");
let unlistenProgress = null;
let statusPollId = null;

function groupBySection(fields) {
  const groups = [];
  const bySection = new Map();
  for (const field of fields) {
    if (!bySection.has(field.section)) {
      const group = { section: field.section, fields: [] };
      bySection.set(field.section, group);
      groups.push(group);
    }
    bySection.get(field.section).fields.push(field);
  }
  return groups;
}

async function loadVersionInfo() {
  versionInfo.value = await invoke("get_version_info");
}

async function loadSettings() {
  settingsError.value = "";
  try {
    const res = await invoke("get_config_settings");
    worldSettings.value = res.worldSettings;
    serverSettings.value = res.serverSettings;
  } catch (error) {
    settingsError.value = String(error);
  }
}

async function refreshServerStatus() {
  const wasGenerated = allFilesGenerated.value;
  const status = await invoke("get_server_status");
  isRunning.value = status.isRunning;
  allFilesGenerated.value = status.allFilesGenerated;
  if (!wasGenerated && status.allFilesGenerated) {
    await loadSettings();
  }
}

onMounted(async () => {
  unlistenProgress = await listen("download-progress", (event) => {
    downloadPercent.value = event.payload.percentage;
  });
  await Promise.all([loadVersionInfo(), refreshServerStatus(), loadSettings()]);
  statusPollId = setInterval(refreshServerStatus, 1500);
});

onUnmounted(() => {
  if (unlistenProgress) {
    unlistenProgress();
  }
  if (statusPollId) {
    clearInterval(statusPollId);
  }
});

async function generateFiles() {
  isGenerating.value = true;
  serverError.value = "";
  try {
    allFilesGenerated.value = await invoke("generate_files");
  } catch (error) {
    serverError.value = String(error);
  } finally {
    isGenerating.value = false;
    await refreshServerStatus();
  }
}

async function startServer() {
  serverError.value = "";
  try {
    isRunning.value = await invoke("start_server");
  } catch (error) {
    serverError.value = String(error);
  }
}

async function stopServer() {
  serverError.value = "";
  try {
    isRunning.value = await invoke("stop_server");
  } catch (error) {
    serverError.value = String(error);
  }
}

async function editGroups() {
  serverError.value = "";
  try {
    await invoke("open_groups_file");
  } catch (error) {
    serverError.value = String(error);
  }
}

async function saveSetting(file, field, newValue) {
  settingsError.value = "";
  field.value = newValue;
  try {
    await invoke("update_config_setting", {
      payload: {
        file,
        path: field.path,
        kind: field.kind,
        value: newValue,
      },
    });
  } catch (error) {
    settingsError.value = String(error);
  }
}

async function updateBinary() {
  isUpdating.value = true;
  downloadPercent.value = 0;
  updateError.value = "";
  try {
    await invoke("install_binary");
    await loadVersionInfo();
  } catch (error) {
    updateError.value = String(error);
  } finally {
    isUpdating.value = false;
  }
}

function handleChangeFolder() {
  menuOpen.value = false;
  emit("back");
}
</script>

<template>
  <div class="config-page">
    <header class="top-bar">
      <div class="menu-wrapper">
        <button
          type="button"
          class="hamburger-btn"
          aria-label="Menu"
          :aria-expanded="menuOpen"
          @click="menuOpen = !menuOpen"
        >
          <span class="bar" />
          <span class="bar" />
          <span class="bar" />
        </button>
        <div v-if="menuOpen" class="dropdown-menu">
          <button type="button" class="menu-item" @click="handleChangeFolder">
            Change Folder
          </button>
        </div>
      </div>
      <h1 class="brand-title">Steel Desktop</h1>
    </header>

    <main class="content">
      <p class="subtitle">Binary installed and ready.</p>
      <div class="info-card">
        <p><strong>Folder:</strong> {{ folderPath }}</p>
        <p><strong>OS:</strong> {{ os }}</p>
        <div class="version-row">
          <span><strong>Version:</strong> {{ versionInfo.installedVersion }}</span>
          <span v-if="versionInfo.isNewest" class="newest-tag">(newest)</span>
          <button
            v-else-if="versionInfo.showUpdate"
            type="button"
            class="update-btn"
            :disabled="isUpdating"
            @click="updateBinary"
          >
            {{
              isUpdating
                ? downloadPercent !== null
                  ? `Updating... ${downloadPercent}%`
                  : "Updating..."
                : "Update"
            }}
          </button>
        </div>
        <p v-if="updateError" class="error-text">{{ updateError }}</p>
      </div>
      <div v-if="!allFilesGenerated" class="warning-box">
        <p class="warning-text">
          Warning: Not all required server files have been generated yet.
        </p>
        <button
          type="button"
          :disabled="isGenerating || isRunning || isUpdating"
          @click="generateFiles"
        >
          {{ isGenerating ? "Generating Files..." : "Generate Files" }}
        </button>
      </div>
      <div class="actions-row">
        <button
          type="button"
          :disabled="isRunning || isUpdating || isGenerating"
          @click="startServer"
        >
          Start
        </button>
        <button type="button" :disabled="!isRunning" @click="stopServer">
          Stop
        </button>
      </div>
      <button type="button" @click="editGroups">Edit Groups</button>
      <p v-if="serverError" class="error-text">{{ serverError }}</p>

      <section class="settings-section">
        <h2>World Settings</h2>
        <p v-if="worldSettings.length === 0" class="empty-hint">
          No world settings found. Generate server files first.
        </p>
        <div
          v-for="group in groupBySection(worldSettings)"
          :key="`world-${group.section}`"
          class="settings-group"
        >
          <h3 v-if="group.section !== 'General'">{{ group.section }}</h3>
          <div
            v-for="field in group.fields"
            :key="field.path.join('.')"
            class="setting-item"
          >
            <div class="setting-meta">
              <span class="setting-label">{{ field.label }}</span>
              <span v-if="field.comment" class="setting-comment">{{ field.comment }}</span>
            </div>
            <label v-if="field.kind === 'boolean'" class="switch">
              <input
                type="checkbox"
                :checked="Boolean(field.value)"
                @change="saveSetting('worlds', field, $event.target.checked)"
              />
              <span class="slider" />
            </label>
            <input
              v-else-if="field.kind === 'integer' || field.kind === 'float'"
              type="number"
              :step="field.kind === 'float' ? 'any' : '1'"
              :value="field.value"
              class="setting-input"
              @change="saveSetting('worlds', field, Number($event.target.value))"
            />
            <input
              v-else
              type="text"
              :value="field.value"
              class="setting-input"
              @change="saveSetting('worlds', field, $event.target.value)"
            />
          </div>
        </div>
      </section>

      <section class="settings-section">
        <h2>Server Settings</h2>
        <p v-if="serverSettings.length === 0" class="empty-hint">
          No server settings found. Generate server files first.
        </p>
        <div
          v-for="group in groupBySection(serverSettings)"
          :key="`server-${group.section}`"
          class="settings-group"
        >
          <h3 v-if="group.section !== 'General'">{{ group.section }}</h3>
          <div
            v-for="field in group.fields"
            :key="field.path.join('.')"
            class="setting-item"
          >
            <div class="setting-meta">
              <span class="setting-label">{{ field.label }}</span>
              <span v-if="field.comment" class="setting-comment">{{ field.comment }}</span>
            </div>
            <label v-if="field.kind === 'boolean'" class="switch">
              <input
                type="checkbox"
                :checked="Boolean(field.value)"
                @change="saveSetting('config', field, $event.target.checked)"
              />
              <span class="slider" />
            </label>
            <input
              v-else-if="field.kind === 'integer' || field.kind === 'float'"
              type="number"
              :step="field.kind === 'float' ? 'any' : '1'"
              :value="field.value"
              class="setting-input"
              @change="saveSetting('config', field, Number($event.target.value))"
            />
            <input
              v-else
              type="text"
              :value="field.value"
              class="setting-input"
              @change="saveSetting('config', field, $event.target.value)"
            />
          </div>
        </div>
      </section>

      <p v-if="settingsError" class="error-text">{{ settingsError }}</p>
    </main>
  </div>
</template>

<style scoped>
.config-page {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  padding: 1.25rem 1.5rem;
  box-sizing: border-box;
}

.top-bar {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 0.85rem;
}

.menu-wrapper {
  position: relative;
}

.hamburger-btn {
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 4px;
  padding: 0.6rem;
  width: 2.5rem;
  height: 2.5rem;
}

.bar {
  display: block;
  width: 100%;
  height: 2px;
  background-color: currentColor;
  border-radius: 2px;
}

.dropdown-menu {
  position: absolute;
  top: calc(100% + 0.4rem);
  left: 0;
  min-width: 160px;
  padding: 0.35rem;
  border-radius: 8px;
  background-color: #ffffff;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
  z-index: 20;
}

.menu-item {
  width: 100%;
  text-align: left;
  box-shadow: none;
  background-color: transparent;
}

.brand-title {
  margin: 0;
  font-size: 1.4rem;
  text-align: left;
}

.content {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: flex-start;
  padding-top: 1.5rem;
  text-align: left;
  gap: 1rem;
}

.subtitle {
  margin: 0;
  opacity: 0.8;
}

.info-card {
  max-width: 520px;
  width: 100%;
  padding: 1rem 1.25rem;
  border-radius: 8px;
  background-color: rgba(255, 255, 255, 0.7);
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.12);
  text-align: left;
  word-break: break-all;
}

.info-card p {
  margin: 0.35rem 0;
}

.version-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin: 0.35rem 0;
}

.newest-tag {
  font-size: 0.85em;
  opacity: 0.7;
}

.update-btn {
  padding: 0.25rem 0.65rem;
  font-size: 0.85em;
}

.error-text {
  color: #d9534f;
  font-size: 0.9em;
}

.warning-box {
  max-width: 520px;
  width: 100%;
  padding: 0.85rem 1.1rem;
  border-radius: 8px;
  border: 1px solid #e6a23c;
  background-color: rgba(230, 162, 60, 0.14);
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.6rem;
}

.warning-text {
  margin: 0;
  font-weight: 500;
}

.actions-row {
  display: flex;
  gap: 0.6rem;
}

button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.settings-section {
  width: 100%;
  max-width: 720px;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  margin-top: 0.75rem;
}

.settings-section h2 {
  margin: 0;
  font-size: 1.25rem;
}

.settings-group {
  width: 100%;
  padding: 0.9rem 1.1rem;
  border-radius: 8px;
  background-color: rgba(255, 255, 255, 0.7);
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.12);
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  box-sizing: border-box;
}

.settings-group h3 {
  margin: 0;
  font-size: 1rem;
  opacity: 0.85;
}

.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}

.setting-meta {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
  flex: 1;
}

.setting-label {
  font-weight: 600;
  font-size: 0.95em;
}

.setting-comment {
  font-size: 0.8em;
  opacity: 0.7;
}

.setting-input {
  width: 240px;
  max-width: 45%;
  padding: 0.45rem 0.65rem;
}

.empty-hint {
  margin: 0;
  font-size: 0.9em;
  opacity: 0.7;
}

.switch {
  position: relative;
  display: inline-block;
  width: 44px;
  height: 24px;
  flex-shrink: 0;
}

.switch input {
  opacity: 0;
  width: 0;
  height: 0;
}

.slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background-color: #9e9e9e;
  border-radius: 24px;
  transition: 0.2s;
}

.slider::before {
  position: absolute;
  content: "";
  height: 18px;
  width: 18px;
  left: 3px;
  bottom: 3px;
  background-color: #ffffff;
  border-radius: 50%;
  transition: 0.2s;
}

.switch input:checked + .slider {
  background-color: #396cd8;
}

.switch input:checked + .slider::before {
  transform: translateX(20px);
}

@media (prefers-color-scheme: dark) {
  .dropdown-menu {
    background-color: #1c1c1c;
  }

  .info-card,
  .settings-group {
    background-color: #0f0f0f98;
  }
}
</style>
