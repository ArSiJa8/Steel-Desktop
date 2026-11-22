<script setup>
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ConfigPage from "./components/ConfigPage.vue";

const folderPath = ref("");
const osOptions = ref([]);
const selectedOs = ref("");
const correctBinaryInstalled = ref(false);
const showInstallPrompt = ref(false);
const isInstalling = ref(false);
const downloadPercent = ref(null);
const statusMsg = ref("");
let unlistenProgress = null;

onMounted(async () => {
  const config = await invoke("get_os_config");
  osOptions.value = config.options;
  selectedOs.value = config.defaultOs;

  unlistenProgress = await listen("download-progress", (event) => {
    downloadPercent.value = event.payload.percentage;
  });
});

onUnmounted(() => {
  if (unlistenProgress) {
    unlistenProgress();
  }
});

async function pickFolder() {
  const selected = await invoke("pick_folder");
  if (typeof selected === "string") {
    folderPath.value = selected;
  }
}

async function selectFolder() {
  const result = await invoke("select_folder", {
    folderPath: folderPath.value,
    os: selectedOs.value,
  });
  correctBinaryInstalled.value = result.correctBinaryInstalled;
  showInstallPrompt.value = result.showInstallPrompt;
  statusMsg.value = result.statusMessage;
}

async function installBinary() {
  isInstalling.value = true;
  downloadPercent.value = 0;
  statusMsg.value = "Installing binary...";
  try {
    const result = await invoke("install_binary");
    correctBinaryInstalled.value = result.correctBinaryInstalled;
    showInstallPrompt.value = result.showInstallPrompt;
    statusMsg.value = result.statusMessage;
  } catch (error) {
    statusMsg.value = String(error);
  } finally {
    isInstalling.value = false;
  }
}

function resetSelection() {
  correctBinaryInstalled.value = false;
  showInstallPrompt.value = false;
  downloadPercent.value = null;
  statusMsg.value = "";
}
</script>

<template>
  <ConfigPage
    v-if="correctBinaryInstalled"
    :folder-path="folderPath"
    :os="selectedOs"
    @back="resetSelection"
  />
  <main v-else class="container">
    <div class="center">
      <h1>Steel Desktop</h1>
      <form v-if="!showInstallPrompt" class="row" @submit.prevent="selectFolder">
        <input
          id="folder-input"
          v-model="folderPath"
          placeholder="Folder path..."
          readonly
          @click="pickFolder"
        />
        <select id="os-select" v-model="selectedOs" aria-label="Operating System">
          <option v-for="os in osOptions" :key="os" :value="os">
            {{ os }}
          </option>
        </select>
        <button type="submit">Select</button>
      </form>
      <p v-if="statusMsg">{{ statusMsg }}</p>
      <div v-if="isInstalling" class="progress-wrapper">
        <progress
          class="progress-bar"
          :value="downloadPercent ?? undefined"
          max="100"
        />
        <span class="progress-text">
          {{ downloadPercent !== null ? `${downloadPercent}%` : "Downloading..." }}
        </span>
      </div>
      <button
        v-if="showInstallPrompt"
        type="button"
        :disabled="isInstalling"
        @click="installBinary"
      >
        {{ isInstalling ? "Installing..." : "Install" }}
      </button>
    </div>
  </main>
</template>

<style scoped>
.center {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

.progress-wrapper {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  margin-bottom: 0.75rem;
}

.progress-bar {
  width: 220px;
  height: 10px;
  accent-color: #396cd8;
}

.progress-text {
  font-size: 0.9em;
  font-weight: 500;
  min-width: 3ch;
}
</style>
<style>
html,
body {
  margin: 0;
  padding: 0;
  height: 100%;
}

:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 24px;
  font-weight: 400;

  color: #0f0f0f;
  background-color: #f6f6f6;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

.container {
  margin: 0;
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
}

.logo {
  height: 6em;
  padding: 1.5em;
  will-change: filter;
  transition: 0.75s;
}

.logo.tauri:hover {
  filter: drop-shadow(0 0 2em #24c8db);
}

.row {
  display: flex;
  justify-content: center;
}

a {
  font-weight: 500;
  color: #646cff;
  text-decoration: inherit;
}

a:hover {
  color: #535bf2;
}

h1 {
  text-align: center;
}

input,
select,
button {
  border-radius: 8px;
  border: 1px solid transparent;
  padding: 0.6em 1.2em;
  font-size: 1em;
  font-weight: 500;
  font-family: inherit;
  color: #0f0f0f;
  background-color: #ffffff;
  transition: border-color 0.25s;
  box-shadow: 0 2px 2px rgba(0, 0, 0, 0.2);
}

button,
select,
#folder-input {
  cursor: pointer;
}

button:hover,
select:hover {
  border-color: #396cd8;
}
button:active {
  border-color: #396cd8;
  background-color: #e8e8e8;
}

input,
select,
button {
  outline: none;
}

#folder-input,
#os-select {
  margin-right: 5px;
}

#os-select {
  padding: 0.6em 0.8em;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f6f6f6;
    background-color: #2f2f2f;
  }

  a:hover {
    color: #24c8db;
  }

  input,
  select,
  button {
    color: #ffffff;
    background-color: #0f0f0f98;
  }
  button:active {
    background-color: #0f0f0f69;
  }
}
</style>
