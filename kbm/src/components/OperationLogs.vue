<template>
  <q-btn flat no-caps icon="receipt_long" label="동작 로그" aria-label="동작 로그" @click="open = true" />
  <q-dialog v-model="open">
    <q-card class="logs-card">
      <q-card-section class="row items-center">
        <div class="text-h6">KBM Control 동작 로그</div><q-space />
        <q-btn v-close-popup flat round icon="close" aria-label="로그 닫기" />
      </q-card-section>
      <q-card-section class="q-gutter-sm">
        <div class="row items-center q-gutter-md">
          <q-toggle :model-value="state?.enabled ?? true" label="파일 기록" :disable="busy" @update:model-value="value => configure(Boolean(value), state?.verbose ?? false)" />
          <q-toggle :model-value="state?.verbose ?? false" label="상세 요청/응답 (진단용)" :disable="busy" @update:model-value="value => configure(state?.enabled ?? true, Boolean(value))" />
          <q-space /><q-btn outline no-caps icon="folder_open" label="로그 폴더 열기" @click="showFolder" />
        </div>
        <div class="text-caption log-path">{{ state?.directory }}</div>
        <div class="text-caption">기본 ON · JSONL · 파일당 4MiB, 최대 5개 · 최근 300건 표시. 이 창의 설정은 이번 실행에 적용됩니다.</div>
        <div class="text-caption text-grey-5">USB/UART·가상 디스크·네트워크 상태와 오류를 기록합니다. 키 입력값, 파일 내용, 패킷 본문은 기록하지 않습니다.</div>
        <div v-if="state?.writeError || error" class="text-negative" role="alert">{{ error || state?.writeError }}</div>
        <div v-if="state?.dropped" class="text-warning">기록하지 못한 로그 {{ state.dropped }}건</div>
        <q-toggle v-model="errorsOnly" label="경고/오류만 보기" dense />
        <div class="log-list" role="log" aria-label="최근 동작 로그">
          <div v-for="(entry, index) in entries" :key="`${entry.time}-${index}`" class="log-entry" :class="entry.level">
            <span>{{ entry.time }} · {{ entry.level.toUpperCase() }} · {{ entry.event }}</span>
            <pre>{{ JSON.stringify(entry.fields) }}</pre>
          </div>
          <div v-if="!entries.length" class="text-grey-5">표시할 로그가 없습니다.</div>
        </div>
      </q-card-section>
    </q-card>
  </q-dialog>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import type { LogState } from '../shared/logging'
const open = ref(false), busy = ref(false), errorsOnly = ref(false), error = ref('')
const state = ref<LogState>()
let timer: ReturnType<typeof setInterval> | undefined
let reading = false
const entries = computed(() => (state.value?.recent ?? []).filter(entry => !errorsOnly.value || entry.level === 'error' || entry.level === 'warn').slice().reverse())
async function refresh(): Promise<void> {
  if (reading) return
  reading = true
  try { state.value = await window.kbmNative.logs.state(); error.value = '' }
  catch (e) { error.value = e instanceof Error ? e.message : String(e) }
  finally { reading = false }
}
async function configure(enabled: boolean, verbose: boolean): Promise<void> {
  busy.value = true
  try { state.value = await window.kbmNative.logs.configure(enabled, verbose) }
  catch (e) { error.value = e instanceof Error ? e.message : String(e) }
  finally { busy.value = false }
}
async function showFolder(): Promise<void> {
  try { await window.kbmNative.logs.open() }
  catch (e) { error.value = e instanceof Error ? e.message : String(e) }
}
watch(open, value => { if (timer) clearInterval(timer); if (value) { void refresh(); timer = setInterval(() => void refresh(), 1000) } })
onBeforeUnmount(() => { if (timer) clearInterval(timer) })
</script>
<style scoped lang="scss">
.logs-card { width: 1100px; max-width: 96vw; background: #101b29; }
.log-path { overflow-wrap: anywhere; }
.log-list { height: 50vh; overflow: auto; font: 12px monospace; background: #070b12; padding: 12px; }
.log-entry { border-bottom: 1px solid #203042; padding: 6px 0; }
.log-entry pre { white-space: pre-wrap; overflow-wrap: anywhere; margin: 3px 0; }
.log-entry.warn { color: #ffcf66; } .log-entry.error { color: #ff8383; }
</style>
