<template>
  <q-expansion-item dense icon="settings_power" label="USB HID System Control · 전체 키" class="system-controls" @before-show="emit('prepare')">
    <div class="text-caption q-my-sm">전용 HID 신호입니다. OS/BIOS가 무시해도 USB는 미지원 응답을 보내지 않습니다. ‘큐 수락’은 실제 동작 성공이 아닙니다. Wake Up도 USB 절전 해제 기능과는 별개입니다.</div>
    <q-select v-model="group" dense outlined dark label="분류" :options="groups" class="q-mb-sm" />
    <div class="system-key-grid">
      <q-btn v-for="control in filtered" :key="control.usage" outline no-caps align="left"
        :color="control.confirm ? 'orange-4' : 'blue-grey-3'" :disable="disabled || pending" @click="choose(control)">
        <span>{{ control.label }}<small>{{ feedbackLabel(control.usage) }}</small></span>
        <q-tooltip>Generic Desktop 01:{{ control.usage.toString(16).toUpperCase() }} · {{ control.confirm ? '별도 확인 후 발신' : 'DOWN/UP 한 번 발신' }}</q-tooltip>
      </q-btn>
    </div>
    <div class="text-caption q-my-sm">System Function Shift Lock Indicator (01:99)는 호스트→장치 표시용 출력이므로 누르는 키가 아닙니다. 일반 배열에 제조사 Fn 키는 추가하지 않습니다.</div>
    <div v-if="last && last.port === port" class="system-result" aria-live="polite">
      <strong>{{ last.label }}</strong> · {{ last.message }}
      <div v-if="last.accepted" class="row items-center q-gutter-xs q-mt-xs">
        <span class="text-caption">영상에서 직접 확인한 결과:</span>
        <q-btn flat dense no-caps label="동작함" @click="mark('works')" />
        <q-btn flat dense no-caps label="안 됨" @click="mark('no-effect')" />
        <q-btn flat dense no-caps label="미확인" @click="mark('unknown')" />
      </div>
    </div>
    <q-dialog v-model="confirmOpen" :persistent="pending">
      <q-card class="system-confirm-card">
        <q-card-section class="text-h6">UART {{ confirmation?.port }} · {{ confirmation?.control.label }}</q-card-section>
        <q-card-section>전원·재시작·디버그·화면 변경 요청으로 작업이 중단되거나 화면이 사라질 수 있습니다. 저장하지 않은 작업을 확인하세요. USB 신호는 물리 RESET 핀을 강제로 누르는 기능이 아니며 호스트의 지원이 필요합니다.</q-card-section>
        <q-card-actions align="right"><q-btn v-close-popup flat label="취소" :disable="pending" /><q-btn color="orange-8" label="확인 후 한 번 발신" :disable="pending || disabled" @click="confirmSend" /></q-card-actions>
      </q-card>
    </q-dialog>
  </q-expansion-item>
</template>
<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { systemControls, type SystemControl } from '../shared/system-controls'
import type { UartPort } from '../services/master-serial'
const props = defineProps<{ port: UartPort; disabled: boolean }>()
const emit = defineEmits<{ prepare: []; busy: [value: boolean]; complete: [] }>()
const group = ref('전체'), pending = ref(false), confirmOpen = ref(false)
const groups = ['전체', ...new Set(systemControls.map(item => item.group))]
const filtered = computed(() => group.value === '전체' ? systemControls : systemControls.filter(item => item.group === group.value))
const confirmation = ref<{ port: UartPort; control: SystemControl }>()
const last = ref<{ port: UartPort; usage: number; label: string; message: string; accepted: boolean }>()
type Feedback = 'works' | 'no-effect' | 'unknown'
const storageKey = 'kbm.system-control-feedback.v1'
// Only HUMAN observations persist; an ACK can never create a success rating.
const ratings = reactive<Record<string, Feedback>>(loadRatings())
const transport = reactive<Record<string, string>>({})
function loadRatings(): Record<string, Feedback> {
  try {
    const data: unknown = JSON.parse(localStorage.getItem(storageKey) ?? '{}')
    if (data && typeof data === 'object' && !Array.isArray(data)) return Object.fromEntries(Object.entries(data).filter(([key, value]) => /^[01]:\d+$/.test(key) && ['works', 'no-effect', 'unknown'].includes(String(value)))) as Record<string, Feedback>
  } catch { /* Optional preference; never blocks input. */ }
  return {}
}
function feedbackLabel(usage: number): string {
  const key = `${props.port}:${usage}`, rating = ratings[key]
  return transport[key] ?? (rating === 'works' ? '동작함 (사용자 확인)' : rating === 'no-effect' ? '안 됨 (사용자 확인)' : '호스트 지원 미확인')
}
async function choose(control: SystemControl): Promise<void> {
  if (props.disabled || pending.value) return
  emit('prepare')
  if (control.confirm) { confirmation.value = { port: props.port, control }; confirmOpen.value = true }
  else await send(props.port, control, false)
}
async function confirmSend(): Promise<void> {
  const selected = confirmation.value
  if (!selected || props.disabled || pending.value) return
  confirmOpen.value = false
  await send(selected.port, selected.control, true)
}
async function send(port: UartPort, control: SystemControl, confirmed: boolean): Promise<void> {
  pending.value = true; emit('busy', true)
  try {
    const result = await window.kbmNative.input.system(port, control.usage, confirmed)
    const key = `${port}:${control.usage}`
    if (result.status === 'accepted') delete transport[key]
    else transport[key] = result.status === 'unsupported' ? '펌웨어 미지원' : 'USB 비활성 · 발신 안 됨'
    last.value = { port, usage: control.usage, label: control.label, message: result.message, accepted: result.status === 'accepted' }
  } catch (error) {
    last.value = { port, usage: control.usage, label: control.label, message: `통신/해제 오류: ${error instanceof Error ? error.message : String(error)}`, accepted: false }
  } finally { pending.value = false; emit('busy', false); emit('complete') }
}
function mark(value: Feedback): void {
  const action = last.value
  if (!action?.accepted || action.port !== props.port) return
  const key = `${action.port}:${action.usage}`
  delete transport[key]; ratings[key] = value
  try { localStorage.setItem(storageKey, JSON.stringify(ratings)) } catch { /* Session rating still works. */ }
}
</script>
<style scoped lang="scss">
.system-key-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(235px, 1fr)); gap: 6px; }
.system-key-grid .q-btn { font-size: 11px; min-height: 46px; }
small { display: block; color: #8b9fb5; font-size: 10px; }
.system-result { padding: 10px; margin-top: 8px; border: 1px solid #34475a; border-radius: 6px; font-size: 12px; }
.system-confirm-card { width: 580px; max-width: 95vw; background: #101b29; }
</style>
