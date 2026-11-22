<template>
  <q-btn flat no-caps icon="keyboard_command_key" label="특수 키" @click="open = true" />
  <q-dialog v-model="open" :persistent="pending">
    <q-card style="width:800px;max-width:95vw;background:#101b29">
      <q-card-section class="row items-center"><div class="text-h6">특수 키 · 키보드 전원 버튼</div><q-space /><q-btn v-close-popup flat round icon="close" :disable="pending" /></q-card-section>
      <q-card-section class="q-gutter-md">
        <q-select v-model="port" :options="ports" emit-value map-options outlined dark label="입력을 보낼 대상 PC" :disable="pending" />
        <div class="text-caption">현재 영상의 UART를 기본 선택합니다. 버튼을 누른 순간의 UART에만 전송하며, 전송 중 화면 매핑이 바뀌어도 같은 대상에서 키를 해제합니다.</div>
        <div class="row q-gutter-sm"><q-btn v-for="preset in specialPresets" :key="preset.label" outline no-caps :label="preset.label" :disable="!available || blocked" @click="sendChord(preset)" /></div>
        <div class="text-caption">로컬 PC에서 Ctrl+Alt+Delete나 Windows 조합을 직접 누를 필요가 없습니다. 각 키를 순서대로 DOWN하고 역순으로 UP합니다.</div>
        <q-separator dark />
        <div class="row items-center q-gutter-sm">
          <q-checkbox v-model="ctrl" label="Ctrl" :disable="blocked" /><q-checkbox v-model="alt" label="Alt" :disable="blocked" />
          <q-checkbox v-model="shift" label="Shift" :disable="blocked" /><q-checkbox v-model="win" label="Win" :disable="blocked" />
        </div>
        <q-select v-model="key" :options="specialKeyOptions" emit-value map-options outlined dark label="조합할 키" :disable="blocked" />
        <q-btn unelevated color="cyan-8" no-caps label="선택한 조합 전송" :disable="!available || blocked" @click="sendChord({ modifiers, key })" />
        <q-separator dark />
        <q-btn outline color="orange-5" no-caps icon="power_settings_new" label="키보드 전원 버튼 누르기…" :disable="!available || blocked" @click="powerTarget = port; confirmPower = true" />
        <div class="text-caption text-orange-3">USB System Power Down 키입니다. 대상 OS 설정에 따라 절전·종료·무시될 수 있습니다. RP2040 재부팅이나 PC 케이스 전원선의 강제 차단 기능이 아닙니다.</div>
        <q-btn flat color="amber-5" no-caps label="대상 입력 모두 해제" :disable="!available || blocked" @click="release" />
        <div v-if="inputState[port]?.needsRelease && !blocked" class="text-warning">해제 여부가 확인되지 않은 입력이 있습니다. 다른 입력 전에 ‘대상 입력 모두 해제’를 눌러주세요.</div>
        <div aria-live="polite">{{ messages[port] }}</div>
        <SystemControls :port="port" :disabled="!available || blocked || inputState[port].needsRelease" @prepare="emit('prepare')" @busy="systemBusy = $event" @complete="refresh" />
        <div v-if="error" class="text-negative" role="alert">{{ error }}</div>
      </q-card-section>
    </q-card>
  </q-dialog>
  <q-dialog v-model="confirmPower" :persistent="pending">
    <q-card style="width:520px;max-width:95vw;background:#101b29">
      <q-card-section class="text-h6">UART {{ powerTarget }} PC에 전원 버튼을 보낼까요?</q-card-section>
      <q-card-section>키보드에 달린 전원 버튼과 같은 요청입니다. 작업 중인 PC가 절전 또는 종료될 수 있습니다. 발신 성공은 OS의 동작 완료를 보장하지 않습니다.</q-card-section>
      <q-card-actions align="right"><q-btn v-close-popup flat label="취소" /><q-btn color="orange-8" label="확인 후 한 번 전송" :disable="pending" @click="sendPower" /></q-card-actions>
    </q-card>
  </q-dialog>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import SystemControls from './SystemControls.vue'
import { masterSerial, type MasterSnapshot, type UartPort } from '../services/master-serial'
import { specialKeyOptions, specialPresets, type KeyChordRequest, type SpecialInputState } from '../shared/special-input'
const props = defineProps<{ activePort: UartPort }>()
const emit = defineEmits<{ prepare: [] }>()
const open = ref(false), pending = ref(false), error = ref(''), confirmPower = ref(false)
const systemBusy = ref(false)
const port = ref<UartPort>(0), powerTarget = ref<UartPort>(0), key = ref('Delete')
const ctrl = ref(true), alt = ref(true), shift = ref(false), win = ref(false)
const modifiers = computed(() => Number(ctrl.value) | Number(alt.value) << 1 | Number(shift.value) << 2 | Number(win.value) << 3)
const ports = [{ label: 'UART 0 PC', value: 0 }, { label: 'UART 1 PC', value: 1 }]
const snapshot = ref<MasterSnapshot>(), messages = ref(['', ''])
const inputState = ref<[SpecialInputState, SpecialInputState]>([{ busy: false, needsRelease: false }, { busy: false, needsRelease: false }])
const blocked = computed(() => pending.value || systemBusy.value || inputState.value[port.value].busy)
const available = computed(() => Boolean(snapshot.value?.connected && snapshot.value.protocolVersion === 3 && snapshot.value.online[port.value] && !masterSerial.isLinkTesting(port.value) && !masterSerial.isRebooting(port.value)))
const unsubscribe = masterSerial.subscribe(value => { snapshot.value = value })
let timer: ReturnType<typeof setInterval> | undefined
async function refresh(): Promise<void> { try { inputState.value = await window.kbmNative.input.state() } catch { /* IPC request reports its own failures. */ } }
async function perform(target: UartPort, action: () => Promise<void>): Promise<void> {
  if (pending.value) return
  pending.value = true; error.value = ''
  try { await action(); messages.value[target] = '요청과 키 해제 수락 완료 · HID 실행/PC 전원 상태는 확인하지 않음' }
  catch (e) { error.value = e instanceof Error ? e.message : String(e) }
  finally { pending.value = false; await refresh() }
}
async function sendChord(request: KeyChordRequest): Promise<void> {
  const target = port.value
  await perform(target, () => window.kbmNative.input.chord(target, { modifiers: request.modifiers, key: request.key }))
}
async function sendPower(): Promise<void> {
  const target = powerTarget.value; confirmPower.value = false
  await perform(target, () => window.kbmNative.input.power(target, true))
}
async function release(): Promise<void> { const target = port.value; await perform(target, () => window.kbmNative.input.release(target)) }
watch(open, value => {
  if (timer) clearInterval(timer)
  if (value) { port.value = props.activePort; emit('prepare'); void refresh(); timer = setInterval(() => void refresh(), 1000) }
})
onBeforeUnmount(() => { unsubscribe(); if (timer) clearInterval(timer) })
</script>
