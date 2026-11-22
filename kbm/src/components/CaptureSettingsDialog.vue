<template>
  <q-btn flat dense round icon="video_settings" aria-label="캡처 해상도 및 장치 순서 설정" @click="openDialog">
    <q-tooltip>캡처 해상도 · 장치 순서</q-tooltip>
  </q-btn>
  <q-dialog v-model="opened" :persistent="busy">
    <q-card class="capture-settings-card">
      <q-card-section class="row items-center">
        <div class="text-h6">캡처 해상도 · 장치 순서</div>
        <q-space />
        <q-btn v-close-popup flat round dense icon="close" aria-label="영상 설정 닫기" :disable="busy" />
      </q-card-section>
      <q-card-section class="q-pt-none text-caption text-grey-5">
        장치 배정과 보드별 영상 모드는 이 PC에 저장되어 다음 실행 때 복원됩니다.
        UART0/1 배정도 캡처보드를 따라 이동합니다. UART 변경은 상단 화면별 선택에서 할 수 있습니다.
        장치 ID는 이 앱의 로컬 식별값이며 보드 고유 시리얼을 보장하지 않습니다.
        USB 포트·앱 데이터가 바뀌면 재배정이 필요할 수 있습니다. 분리된 보드의 자리는 유지합니다.
      </q-card-section>
      <q-card-section class="q-pt-none">
        <div v-for="index in [0, 1] as const" :key="index" class="capture-assignment-row">
          <q-select v-model="draftAssignments[index]" :options="assignmentOptions(index)" emit-value map-options
            dark outlined dense :label="`화면 ${index + 1} 장치`" :disable="busy" />
        </div>
        <q-btn outline no-caps color="cyan-4" label="장치 순서 저장 및 적용" :disable="busy || !assignmentValid"
          @click="emit('assign', [...draftAssignments])" />
        <q-btn class="q-ml-sm" flat no-caps label="화면 1 ↔ 2 바꾸기" :disable="busy"
          @click="draftAssignments = [draftAssignments[1], draftAssignments[0]]" />
      </q-card-section>
      <q-separator dark />
      <q-card-section class="text-caption text-grey-5">
        목록은 일반적인 모드 후보이며 지원이 보장된 목록은 아닙니다. 적용 시 해당 보드만 닫고 다시 열어
        첫 프레임을 확인합니다. 지원하지 않으면 오류를 표시하고 이전 영상 모드로 복원을 시도합니다.
        자동 탐색은 1080p → 720p → 480p → 기본, 각각 30 → 24 → 기본 FPS 순입니다.
      </q-card-section>
      <q-card-section v-for="index in [0, 1] as const" :key="index" class="capture-mode-section">
        <div class="text-subtitle1">화면 {{ index + 1 }} · {{ deviceFor(index)?.label ?? '저장된 장치 미연결' }}</div>
        <div class="text-caption text-cyan-4">제어 대상: UART {{ uartPorts[index] }} · 캡처보드에 저장된 배정</div>
        <div class="text-caption text-grey-5 capture-identity">장치 ID: {{ assignments[index] ?? '아직 배정되지 않음' }}</div>
        <div class="text-caption q-mb-sm">
          실제 수신: {{ actualLabel(index) }}
          <span v-if="savedMode(index).kind === 'auto'"> · 저장: 자동 탐색</span>
        </div>
        <q-select v-model="drafts[index].choice" :options="modeOptions(index)" emit-value map-options
          dark outlined dense :label="`화면 ${index + 1} 영상 모드 (W x H / Hz)`" :disable="busy || !deviceFor(index)" />
        <div v-if="drafts[index].choice === 'manual'" class="capture-manual-row">
          <q-input v-model.number="drafts[index].width" type="number" dark outlined dense label="너비 (W)"
            :min="2" :max="8192" :step="1" :disable="busy" />
          <q-input v-model.number="drafts[index].height" type="number" dark outlined dense label="높이 (H)"
            :min="2" :max="8192" :step="1" :disable="busy" />
          <q-input v-model.number="drafts[index].frameRate" type="number" dark outlined dense label="목표 Hz"
            :min="1" :max="240" :step="0.001" :disable="busy" />
        </div>
        <div v-if="!selectionFor(index)" class="text-caption text-orange-4 q-mt-sm">
          너비·높이는 2..8192 정수, 목표 Hz는 1..240을 입력하세요. 예: 59.94
        </div>
        <q-btn class="q-mt-sm" unelevated no-caps color="cyan-8" :label="`화면 ${index + 1} 모드 저장 및 적용`"
          :loading="busy" :disable="busy || !deviceFor(index) || !selectionFor(index)" @click="applyMode(index)" />
      </q-card-section>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import type { CaptureSlot, VideoDeviceInventory } from '../services/capture-devices'
import {
  CAPTURE_MODE_PRESETS, loadCaptureMode, modeKey, modeLabel, validAssignments, validCaptureMode,
  type CaptureAssignments, type CaptureMode, type FixedCaptureMode, type ScreenUartPorts
} from '../services/capture-preferences'

const props = defineProps<{
  inventory: VideoDeviceInventory[]
  assignments: CaptureAssignments
  captures: Array<CaptureSlot | undefined>
  uartPorts: ScreenUartPorts
  busy: boolean
}>()
const emit = defineEmits<{
  prepare: []
  assign: [assignments: CaptureAssignments]
  mode: [index: 0 | 1, mode: CaptureMode]
}>()
const opened = ref(false)
const draftAssignments = ref<CaptureAssignments>([null, null])
interface ModeDraft { choice: string; width: number | string; height: number | string; frameRate: number | string }
const drafts = reactive<[ModeDraft, ModeDraft]>([
  { choice: 'auto', width: 1920, height: 1080, frameRate: 30 },
  { choice: 'auto', width: 1920, height: 1080, frameRate: 30 }
])
const assignmentValid = computed(() => validAssignments(draftAssignments.value))
function deviceFor(index: 0 | 1): VideoDeviceInventory | undefined {
  return props.inventory.find(device => !device.blacklisted && device.deviceId === props.assignments[index])
}
function savedMode(index: 0 | 1): CaptureMode {
  const id = props.assignments[index]
  return id ? loadCaptureMode(id) : { kind: 'auto' }
}
function fixedOptions(index: 0 | 1): FixedCaptureMode[] {
  const modes = [...CAPTURE_MODE_PRESETS]
  const saved = savedMode(index)
  const capture = props.captures[index]
  if (saved.kind === 'fixed') modes.push(saved)
  const actual: FixedCaptureMode = { kind: 'fixed', width: capture?.width ?? 0,
    height: capture?.height ?? 0, frameRate: capture?.frameRate ?? 0 }
  if (validCaptureMode(actual)) modes.push(actual)
  return [...new Map(modes.map(mode => [modeKey(mode), mode])).values()]
}
function modeOptions(index: 0 | 1): Array<{ label: string; value: string }> {
  return [{ label: '자동 탐색', value: 'auto' },
    ...fixedOptions(index).map(mode => ({ label: modeLabel(mode), value: modeKey(mode) })),
    { label: '수동 지정', value: 'manual' }]
}
function selectionFor(index: 0 | 1): CaptureMode | undefined {
  const draft = drafts[index]
  if (draft.choice === 'auto') return { kind: 'auto' }
  if (draft.choice !== 'manual') return fixedOptions(index).find(mode => modeKey(mode) === draft.choice)
  const mode = { kind: 'fixed', width: Number(draft.width), height: Number(draft.height), frameRate: Number(draft.frameRate) }
  return validCaptureMode(mode) ? mode : undefined
}
function assignmentOptions(index: 0 | 1): Array<{ label: string; value: string | null; disable?: boolean }> {
  const other = draftAssignments.value[index === 0 ? 1 : 0]
  const options: Array<{ label: string; value: string | null; disable?: boolean }> = [
    { label: '미배정 (다음 검색 시 빈자리에 장치 배정)', value: null },
    ...props.inventory.filter(device => !device.blacklisted).map((device, position) => ({
      label: `${position + 1}. ${device.label} · ID ${device.deviceId.slice(-12)}`,
      value: device.deviceId, disable: device.deviceId === other
    }))
  ]
  const saved = draftAssignments.value[index]
  if (saved && !options.some(option => option.value === saved)) {
    options.push({ label: `미연결 장치 · ID ${saved.slice(-12)} (자리 유지)`, value: saved })
  }
  return options
}
function actualLabel(index: 0 | 1): string {
  const slot = props.captures[index]
  return slot ? modeLabel({ kind: 'fixed', width: slot.width, height: slot.height, frameRate: slot.frameRate }) : '영상 없음'
}
function syncDrafts(): void {
  draftAssignments.value = [...props.assignments]
  for (const index of [0, 1] as const) {
    const saved = savedMode(index)
    drafts[index].choice = saved.kind === 'auto' ? 'auto' : modeKey(saved)
    if (saved.kind === 'fixed') Object.assign(drafts[index], saved)
  }
}
function openDialog(): void { syncDrafts(); emit('prepare'); opened.value = true }
function applyMode(index: 0 | 1): void {
  const mode = selectionFor(index)
  if (mode && !props.busy) emit('mode', index, mode)
}
watch(() => props.assignments, () => { if (opened.value) syncDrafts() })
</script>

<style scoped lang="scss">
.capture-settings-card { width: 820px; max-width: 95vw; background: #0d141f; }
.capture-assignment-row { margin-bottom: 12px; }
.capture-mode-section { border-top: 1px solid #263447; }
.capture-identity { overflow-wrap: anywhere; margin-bottom: 6px; font-family: monospace; }
.capture-manual-row { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin-top: 12px; }
@media (max-width: 600px) { .capture-manual-row { grid-template-columns: 1fr; } }
</style>
