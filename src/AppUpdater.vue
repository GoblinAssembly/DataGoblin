<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue';
import { isTauri } from '@tauri-apps/api/core';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

const props = defineProps<{ blocked: boolean; beforeInstall: () => Promise<void> }>();
const desktop = isTauri();
const state = ref<'idle' | 'checking' | 'downloading' | 'ready' | 'installing' | 'error'>('idle');
const message = ref('Check for updates');
let update: Update | null = null;
let disposed = false;
let timer: ReturnType<typeof setInterval> | undefined;
function blockKeys(event: KeyboardEvent) {
  if (state.value !== 'installing') return;
  event.preventDefault();
  event.stopImmediatePropagation();
}
async function checkUpdate() {
  if (!desktop || disposed || ['checking', 'downloading', 'ready', 'installing'].includes(state.value)) return;
  state.value = 'checking';
  message.value = 'Checking for updates…';
  try {
    await update?.close();
    update = await check({ timeout: 15000 });
    if (disposed) { await update?.close(); return; }
    if (!update) { state.value = 'idle'; message.value = 'DataGoblin is up to date'; return; }
    state.value = 'downloading';
    message.value = `Downloading ${update.version}…`;
    let total = 0, received = 0;
    await update.download(event => {
      if (event.event === 'Started') total = event.data.contentLength ?? 0;
      if (event.event === 'Progress') {
        received += event.data.chunkLength;
        message.value = total ? `Downloading update ${Math.min(100, Math.round(received / total * 100))}%` : 'Downloading update…';
      }
    }, { timeout: 120000 });
    state.value = 'ready';
    message.value = `Restart to update to ${update.version}`;
  } catch (error) {
    state.value = 'error';
    message.value = `Update unavailable. Click to retry. ${String(error)}`;
  }
}
async function install() {
  if (!update || props.blocked || state.value !== 'ready') return;
  state.value = 'installing';
  try {
    await props.beforeInstall();
    if (props.blocked) throw new Error('Finish queries and apply or revert grid edits first.');
    await update.install();
    await relaunch();
  } catch (error) {
    state.value = 'ready';
    message.value = `Could not restart: ${String(error)}. Click to retry.`;
  }
}
onMounted(() => {
  window.addEventListener('keydown', blockKeys, true);
  if (!desktop || import.meta.env.DEV) return;
  void checkUpdate();
  timer = setInterval(() => void checkUpdate(), 4 * 60 * 60 * 1000);
});
onBeforeUnmount(() => {
  window.removeEventListener('keydown', blockKeys, true);
  disposed = true;
  clearInterval(timer);
  void update?.close().catch(() => {});
});
</script>

<template>
  <button v-if="desktop" class="update-button"
    :title="state === 'ready' && blocked ? 'Finish queries and apply or revert grid edits before updating.' : message"
    :disabled="['checking', 'downloading', 'installing'].includes(state) || (state === 'ready' && blocked)"
    @click="state === 'ready' ? install() : checkUpdate()">
    {{ state === 'ready' ? 'Restart to update' : state === 'downloading' ? message : state === 'checking' ? 'Checking…' : state === 'installing' ? 'Updating…' : state === 'error' ? 'Retry update' : 'Updates' }}
  </button>
  <Teleport to="body">
    <div v-if="state === 'installing'" class="update-overlay" role="alert" aria-live="assertive">Saving queries and updating DataGoblin…</div>
  </Teleport>
</template>

<style scoped>
.update-button {
  font-size: 12px;
  line-height: 16px;
  border: 1px solid transparent;
  border-radius: 6px;
  padding: 7px 10px;
  white-space: nowrap;
}
.update-button:disabled { opacity: .36; cursor: not-allowed; }
.update-overlay { position: fixed; inset: 0; z-index: 10000; display: grid; place-items: center; background: #101827ee; color: white; }
</style>
