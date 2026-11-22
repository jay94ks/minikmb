<template>
  <aside v-show="visible" class="virtual-keyboard" aria-label="가상 키보드" @pointerdown.stop="emit('prepare')" @pointermove.stop @wheel.stop @keydown.stop @keyup.stop>
    <div class="keyboard-heading">
      <q-icon name="keyboard" size="20px" /><strong>가상 키보드</strong><q-badge color="cyan-9">화면 {{ screen }} → UART {{ port }}</q-badge>
      <span class="keyboard-hint">조합 이름을 클릭하거나 보조키 선택 후 키를 클릭</span><q-space />
      <q-btn flat dense round icon="close" aria-label="가상 키보드 닫기" @click="emit('close')" />
    </div>
    <div class="keyboard-scroll">
      <div class="keyboard-presets" aria-label="특수 키 조합">
        <q-btn v-for="preset in specialPresets" :key="preset.label" dense outline no-caps :label="preset.label" :disable="!available || blocked" @click="send(preset)" />
      </div>
      <div class="keyboard-modifiers">
        <q-btn v-for="modifier in virtualModifiers" :key="modifier.mask" dense no-caps :label="modifier.label" :aria-pressed="Boolean(modifiers & modifier.mask)" :color="modifiers & modifier.mask ? 'cyan-8' : 'blue-grey-9'" :disable="blocked" @click="modifiers ^= modifier.mask" />
        <q-btn flat dense no-caps label="보조키 해제" :disable="blocked || !modifiers" @click="modifiers = 0" />
        <span class="keyboard-hint">보조키는 다음 키에 한 번 적용 · 실제 키를 누른 채 유지하지 않음</span>
        <q-space /><q-btn flat dense no-caps color="amber-5" label="대상 입력 모두 해제" :disable="!available || blocked" @click="release" />
      </div>
      <div v-for="(row, index) in virtualKeyboardRows" :key="index" class="keyboard-row">
        <q-btn v-for="key in row" :key="key.code" dense no-caps :label="key.label" :style="{ flexGrow: key.width ?? 1 }" :disable="!available || blocked || inputState[port].needsRelease" @click="send({ modifiers, key: key.code })" />
      </div>
      <div v-if="inputState[port].needsRelease" class="text-warning text-caption q-mt-sm">키 해제 미확인 · 새 입력 전에 ‘대상 입력 모두 해제’를 눌러주세요.</div>
      <div v-if="message" class="keyboard-message" aria-live="polite">{{ message }}</div>
      <SystemControls :port="port" :disabled="!available || blocked || inputState[port].needsRelease" @prepare="prepareSystem" @busy="systemBusy = $event" @complete="refresh" />
    </div>
  </aside>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { masterSerial, type MasterSnapshot, type UartPort } from '../services/master-serial'
import { specialPresets, type KeyChordRequest, type SpecialInputState } from '../shared/special-input'
import { virtualKeyboardRows, virtualModifiers } from '../shared/virtual-keyboard'
import SystemControls from './SystemControls.vue'
const props = defineProps<{ visible: boolean; port: UartPort; screen: number }>()
const emit = defineEmits<{ close: []; prepare: [] }>()
const modifiers = ref(0), pending = ref(false), systemBusy = ref(false), message = ref('')
const snapshot = ref<MasterSnapshot>()
const inputState = ref<[SpecialInputState, SpecialInputState]>([{ busy: false, needsRelease: false }, { busy: false, needsRelease: false }])
const blocked = computed(() => pending.value || systemBusy.value || inputState.value[props.port].busy)
const available = computed(() => Boolean(snapshot.value?.connected && snapshot.value.protocolVersion === 3 && snapshot.value.online[props.port] && !masterSerial.isLinkTesting(props.port) && !masterSerial.isRebooting(props.port)))
const unsubscribe = masterSerial.subscribe(value => { snapshot.value = value })
let timer: ReturnType<typeof setInterval> | undefined
async function refresh(): Promise<void> { try { inputState.value = await window.kbmNative.input.state() } catch { /* Action reports IPC failures. */ } }
async function perform(action: () => Promise<void>): Promise<void> {
  if (!available.value || blocked.value) return
  pending.value = true; message.value = ''; emit('prepare')
  try { await action(); message.value = 'DOWN/UP 큐 수락 · 실제 OS 동작은 미확인' }
  catch (error) { message.value = error instanceof Error ? error.message : String(error) }
  finally { pending.value = false; await refresh() }
}
async function send(request: KeyChordRequest): Promise<void> {
  if (inputState.value[props.port].needsRelease) return
  const target = props.port, chord = { modifiers: request.modifiers, key: request.key }
  modifiers.value = 0
  await perform(() => window.kbmNative.input.chord(target, chord))
}
async function release(): Promise<void> { const target = props.port; modifiers.value = 0; await perform(() => window.kbmNative.input.release(target)) }
function prepareSystem(): void { emit('prepare'); if (document.fullscreenElement) void document.exitFullscreen() }
watch(() => [props.port, props.visible], () => { modifiers.value = 0; message.value = '' })
watch(() => props.visible, value => {
  if (timer) clearInterval(timer)
  if (value) { void refresh(); timer = setInterval(() => void refresh(), 500) }
}, { immediate: true })
onBeforeUnmount(() => { unsubscribe(); if (timer) clearInterval(timer) })
</script>
<style scoped lang="scss">
.virtual-keyboard { position: absolute; z-index: 5; bottom: 10px; left: 12px; right: 12px; max-width: 1120px; margin: auto; border: 1px solid #3c5267; border-radius: 10px; background: rgba(11, 20, 31, .96); box-shadow: 0 -10px 45px #0008; color: #e4edf7; }
.keyboard-heading { display: flex; align-items: center; gap: 10px; padding: 7px 12px; border-bottom: 1px solid #293a4c; font-size: 12px; }
.keyboard-scroll { padding: 8px 12px 10px; max-height: min(50vh, 440px); overflow: auto; }
.keyboard-presets, .keyboard-modifiers { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-bottom: 8px; }
.keyboard-presets .q-btn { font-size: 11px; color: #8ee3eb; }
.keyboard-modifiers .q-btn { font-size: 11px; }
.keyboard-row { display: flex; gap: 4px; margin-bottom: 4px; }
.keyboard-row .q-btn { flex: 1 1 0; min-width: 0; min-height: 29px; padding: 2px; border: 1px solid #3d4e62; background: #1b293a; font-size: 11px; }
.keyboard-row .q-btn:hover { background: #25455d; }
.keyboard-hint { color: #899db4; font-size: 10px; }
.keyboard-message { padding: 5px 0; color: #b2d2e4; font-size: 11px; }
@media (max-width: 1100px) { .keyboard-heading .keyboard-hint, .keyboard-modifiers .keyboard-hint { display: none; } }
</style>
