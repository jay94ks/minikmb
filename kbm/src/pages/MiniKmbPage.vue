<template>
  <q-layout view="hHh Lpr fFf" class="mini-shell" :class="{ 'control-shell': controlMode }">
    <q-header class="mini-header">
      <q-toolbar>
        <q-toolbar-title><strong>{{ controlMode ? channelTitle(active) : '채널 목록' }}</strong><small>{{ controlMode ? `${uidHex(selected)} · ${targetLabel}` : `${state.boards.length} 보드 · ${channels.length} 채널` }}</small></q-toolbar-title>
        <q-btn v-if="controlMode" flat icon="arrow_back" label="채널 목록" @click="backToChannels" />
        <q-btn v-if="controlMode && active" flat dense icon="edit" label="이름 변경" :disable="renaming || active.nameSupported === false" @click="renameChannel(active)" />
        <q-btn flat icon="refresh" label="장치 검색" :loading="scanning" @click="rescan" />
        <div v-if="controlMode" class="toolbar-video-controls">
          <q-select class="video-mode" dark dense outlined emit-value map-options v-model="mode" label="영상 모드" :display-value="typeof mode === 'object' ? `${mode.width}×${mode.height} / ${mode.frameRate} FPS` : modeOptions.find(o => o.value === mode)?.label" :options="modeOptions" :disable="busy" />
          <q-btn flat dense color="cyan-4" icon="play_arrow" label="영상 열기" :disable="!active?.captureId" :loading="busy" @click="openVideo" />
          <q-btn flat dense icon="stop" label="중지" :disable="!capture && !busy" @click="stopVideo" />
          <q-btn flat dense label="수동 W/H/Hz" @click="customMode" />
        </div>
      </q-toolbar>
      <div v-if="controlMode" class="console-toolbar">
        <q-btn flat dense icon="keyboard" label="가상 키보드 열기" @click="tools?.show('keyboard')" />
        <q-checkbox :model-value="specialKeys" label="특수 키 전송" :title="specialKeys ? (nativeKeyboard ? '키 입력은 타깃에만 전달됩니다.' : '영상에 포커스를 두면 타깃으로 전달합니다.') : undefined" dense @update:model-value="setSpecialKeys" />
        <q-btn flat dense icon="terminal" label="시리얼 중계" @click="tools?.show('serial')" />
        <q-btn flat dense icon="lan" label="네트워크 · 포워딩" @click="tools?.show('network')" />
        <q-btn flat dense icon="folder_shared" label="디스크 공유" @click="tools?.show('storage')" />
        <q-btn flat dense icon="power_settings_new" label="PC 깨우기" @click="tools?.show('wake')" />
        <q-btn flat dense icon="monitor_heart" label="진단" @click="tools?.show('diagnostics')" /><OperationLogs />
      </div>
    </q-header>
    <q-drawer v-if="!controlMode" show-if-above :width="280" bordered class="mini-nav">
      <div class="mini-nav-heading">CONNECTED BOARDS</div>
      <div v-if="!state.boards.length" class="q-pa-lg text-grey-5">연결된 MCU를 검색하고 있습니다.<br>보드의 관리 USB를 연결하세요.</div>
      <section v-for="board in state.boards" :key="board.id" class="mini-board">
        <div class="mini-board-title"><q-icon name="developer_board" color="cyan-4" /> {{ board.label }}</div>
        <q-item v-for="(channel, index) in board.channels" :key="channel.uid" clickable :active="selected === channel.uid"
          active-class="mini-selected" @click="choose(channel.uid)">
          <q-item-section avatar><q-icon name="desktop_windows" :color="channel.connected ? 'cyan-4' : 'grey-7'" /></q-item-section>
          <q-item-section><q-item-label>{{ channelTitle(channel) }}</q-item-label><q-item-label caption>{{ uidHex(channel.uid) }}</q-item-label></q-item-section>
          <q-item-section side><q-icon :name="channel.captureId ? 'link' : 'link_off'" size="18px" /></q-item-section>
        </q-item>
        <div v-if="board.pairing" class="mini-board-meta">{{ board.pairing }}</div>
      </section>
    </q-drawer>
    <q-page-container><q-page class="mini-content" :class="{ 'console-page': controlMode }">
      <q-banner v-if="state.error" rounded class="bg-orange-10 q-mb-md">{{ state.error }}</q-banner>
      <q-banner v-if="error" rounded class="bg-red-10 q-mb-md">{{ error }}</q-banner>
      <template v-if="!controlMode">
        <div class="mini-eyebrow">CHANNEL MANAGER</div><h1>제어할 채널을 선택하세요</h1>
        <div class="channel-grid"><q-card v-for="channel in channels" :key="channel.uid" flat bordered class="channel-card" :class="{ 'selected-card': selected === channel.uid }" @click="choose(channel.uid)">
          <q-card-section><div class="row items-center"><q-icon name="desktop_windows" size="28px" color="cyan-4" /><q-space /><q-chip dense :color="channel.connected ? 'teal-9' : 'grey-9'">{{ channel.connected ? '연결됨' : '미연결' }}</q-chip></div>
            <h2>{{ channelTitle(channel) }}</h2><div class="text-caption mono">{{ uidHex(channel.uid) }}</div><p v-if="channel.pairingError" class="text-caption text-orange">{{ channel.pairingError }}</p>
            <div v-if="channel.nameError" class="text-caption text-orange">{{ channel.nameError }}</div>
            <q-chip dense outline>{{ ['타깃 미연결', '타깃 FS', '타깃 HS', '타깃 절전'][channel.targetState] }}</q-chip>
          </q-card-section><q-card-actions align="right"><q-btn flat icon="edit" label="이름 변경" :disable="renaming || !channel.connected || channel.nameSupported === false" @click.stop="renameChannel(channel)" /><q-btn unelevated color="cyan-9" icon="open_in_new" label="제어하기" :disable="!channel.connected" @click.stop="enterControl(channel.uid)" /></q-card-actions>
        </q-card></div>
        <q-banner v-if="!channels.length" class="mini-notice q-mt-lg">보드의 관리 USB를 연결한 뒤 장치 검색을 눌러주세요.</q-banner>
      </template>
      <template v-else>
      <q-banner v-if="active?.pairingError" rounded class="mini-notice">{{ active.pairingError }}</q-banner>
      <div ref="stage" class="mini-stage" tabindex="0" @keydown="keyDown" @keyup="keyUp" @focus="stageFocus" @blur="stageBlur"
        @pointerdown="pointerDown" @pointerup="pointerUp" @pointercancel="releaseInputs" @lostpointercapture="lostCapture" @pointermove="pointerMove"
        @wheel.prevent="wheel" @contextmenu.prevent>
        <canvas v-if="capture" ref="canvas" class="mini-video" />
        <div v-else class="mini-placeholder"><q-icon name="videocam_off" size="64px" /><h2>{{ busy ? '영상 연결 중…' : '영상이 연결되지 않았습니다' }}</h2><p v-if="!busy">상단에서 영상을 여세요.</p></div>
      </div>
      <q-banner v-if="active?.error" rounded class="bg-red-10 q-mt-md">{{ active.error }}</q-banner>
      <q-banner v-if="active && !(active.features & 1) && active.connected" rounded class="mini-notice q-mt-md">현재 MCU 펌웨어에 HID 기능이 없습니다.</q-banner>
      <MiniChannelTools v-if="active?.connected" ref="tools" :key="active.uid" :uid="active.uid" :before-input="releaseInputs" console />
      </template>
    </q-page></q-page-container>
  </q-layout>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import { useRoute, useRouter, onBeforeRouteLeave } from 'vue-router'
import { useQuasar } from 'quasar'
import { channelName, uidHex, type MiniChannel, type MiniInventory, type MiniInput } from '../shared/minikmb'
import type { MiniVideoInfo, MiniVideoMode, MiniVideoFrame } from '../shared/mini-capture'
import { miniKeyboardUsage, isMiniSpecialKey } from '../services/mini-keyboard'
import MiniChannelTools from '../components/MiniChannelTools.vue'
import OperationLogs from '../components/OperationLogs.vue'
import { absolutePoint, AbsolutePointer } from '../services/absolute-pointer'
const $q = useQuasar(), api = window.kbmNative.mini
const route = useRoute(), router = useRouter(), controlMode = computed(() => route.name === 'mini-control')
const tools = ref<InstanceType<typeof MiniChannelTools>>()
const state = shallowRef<MiniInventory>({ boards: [], error: '', scannedAt: 0 })
const capture = shallowRef<MiniVideoInfo>()
const selected = ref(''), scanning = ref(false), busy = ref(false), windowFocused = ref(true), error = ref(''), mode = ref<MiniVideoMode>('auto')
const specialKeys = ref(false), renaming = ref(false)
const stageFocused = ref(false), nativeKeyboard = ref(false)
let keyboardRequest = 0
async function syncKeyboard(): Promise<void> {
  const ticket = ++keyboardRequest, uid = selected.value
  const enabled = specialKeys.value && stageFocused.value && canInput.value
  nativeKeyboard.value = false
  try {
    const result = await api.keyboard.capture(uid, Boolean(enabled))
    if (ticket === keyboardRequest && uid === selected.value) nativeKeyboard.value = result.active
  } catch (e) {
    if (ticket === keyboardRequest) { specialKeys.value = false; error.value = String(e) }
  }
}
function stageFocus(): void { stageFocused.value = true; void syncKeyboard() }
function stageBlur(): void { stageFocused.value = false; void releaseInputs() }
const channelTitle = (channel?: MiniChannel) => channel ? `채널 ${channel.slot ?? '—'}${channel.name ? ` · ${channel.name}` : ''}` : '연결된 채널 없음'
function renameChannel(channel: MiniChannel): void {
  const uid = channel.uid
  $q.dialog({ title: `${channelTitle(channel)} 이름 변경`, message: 'SD 카드에 저장합니다. UTF-8 64바이트, 빈 이름은 기본 표시로 되돌립니다.', prompt: { model: channel.name ?? '', type: 'text' }, cancel: true }).onOk(async (value: string) => {
    renaming.value = true; error.value = ''
    try { const name = channelName(value); await api.tools.rename(uid, name); state.value = await api.state(); $q.notify({ type: 'positive', message: '채널 이름을 SD 카드에 저장했습니다.' }) }
    catch (e) { error.value = String(e); $q.notify({ type: 'negative', message: String(e), timeout: 7000 }) }
    finally { renaming.value = false }
  })
}
async function setSpecialKeys(value: boolean): Promise<void> {
  const uid = selected.value
  specialKeys.value = false
  await releaseInputs()
  if (controlMode.value && selected.value === uid) { specialKeys.value = value === true; stage.value?.focus(); await syncKeyboard() }
}
const stage = ref<HTMLElement>(), canvas = ref<HTMLCanvasElement>()
const channels = computed(() => state.value.boards.flatMap(b => b.channels))
const active = computed(() => channels.value.find(c => c.uid === selected.value))
const canInput = computed(() => controlMode.value && windowFocused.value && active.value?.connected && Boolean(active.value.features & 1) && [1, 2].includes(active.value.targetState) && Boolean(capture.value))
const targetLabel = computed(() => ['타깃 미연결', '타깃 FS', '타깃 HS', '타깃 절전'][active.value?.targetState ?? 0])
const modeOptions = [{ label: '자동 탐색', value: 'auto' }, { label: '1920×1080 / 30 FPS', value: '1080' }, { label: '1280×720 / 30 FPS', value: '720' }, { label: '640×480 / 30 FPS', value: '480' }]
let epoch = 0, selectionEpoch = 0, inputGeneration = 0, disposed = false
let unsubscribe = () => {}, unsubscribeFrame = () => {}, unsubscribeEnded = () => {}, unsubscribeKeyboard = () => {}
const heldKeys = new Map<string, number>(), heldButtons = new Set<number>()
const pointer = new AbsolutePointer(send, e => { error.value = String(e); void releaseInputs() })
async function drawFrame(frame: MiniVideoFrame): Promise<void> {
  try {
    if (disposed || frame.uid !== selected.value || frame.requestId !== epoch) return
    capture.value = frame; await nextTick()
    const bitmap = await createImageBitmap(new Blob([Uint8Array.from(frame.jpeg).buffer], { type: 'image/jpeg' }))
    try {
      if (disposed || frame.uid !== selected.value || frame.requestId !== epoch || !canvas.value) return
      canvas.value.width = frame.width; canvas.value.height = frame.height
      canvas.value.getContext('2d')?.drawImage(bitmap, 0, 0)
    } finally { bitmap.close() }
  } catch (e) { error.value = `영상 표시 오류: ${String(e)}` }
  finally { api.video.ack(frame.token, frame.sequence) }
}
async function releaseInputs(): Promise<void> {
  ++keyboardRequest; nativeKeyboard.value = false
  ++inputGeneration; pointer.cancel(); heldKeys.clear(); heldButtons.clear()
  await api.keyboard.capture(selected.value, false).catch(() => {})
  await api.releaseInput().catch(e => { error.value = String(e) })
}
async function stopVideo(): Promise<void> {
  const ticket = ++epoch; await releaseInputs()
  await api.video.stop()
  if (ticket === epoch) { capture.value = undefined; busy.value = false }
}
async function openVideo(): Promise<void> {
  const ticket = ++epoch, uid = selected.value
  await releaseInputs()
  if (ticket !== epoch || disposed) return
  capture.value = undefined; busy.value = true; error.value = ''
  try {
    const next = await api.video.start(uid, mode.value, ticket)
    if (!disposed && ticket === epoch) { capture.value = next; localStorage.setItem(`minikmb.video.${uid}`, JSON.stringify(mode.value)); await nextTick(); if (windowFocused.value) stage.value?.focus() }
  } catch (e) { if (ticket === epoch) error.value = String(e) }
  finally { if (ticket === epoch) busy.value = false }
}
async function choose(uid: string): Promise<void> {
  if (selected.value === uid) return
  const ticket = ++selectionEpoch
  await stopVideo()
  if (ticket !== selectionEpoch || disposed) return
  selected.value = uid
  restoreMode(uid)
  if (controlMode.value && active.value?.captureId) await openVideo()
}
async function enterControl(uid: string): Promise<void> { await stopVideo(); await router.push({ name: 'mini-control', params: { uid } }) }
async function backToChannels(): Promise<void> { await stopVideo(); await api.tools.deactivate(selected.value); await router.push('/') }
async function rescan(): Promise<void> {
  if (scanning.value) return
  scanning.value = true
  try {
    await stopVideo()
    const snapshot = await api.scan()
    if (disposed) return
    state.value = snapshot
    if (controlMode.value) { selected.value = String(route.params.uid ?? ''); restoreMode(selected.value) }
    else if (!channels.value.some(c => c.uid === selected.value)) { selected.value = channels.value[0]?.uid ?? ''; restoreMode(selected.value) }
    if (controlMode.value && active.value?.captureId) await openVideo()
  } catch (e) { error.value = String(e) }
  finally { scanning.value = false }
}
function restoreMode(uid: string): void {
  mode.value = 'auto'
  try { const saved: MiniVideoMode = JSON.parse(localStorage.getItem(`minikmb.video.${uid}`) ?? 'null'); if (saved) mode.value = saved } catch { /* Keep automatic mode. */ }
}
function customMode(): void {
  $q.dialog({ title: '수동 영상 모드', message: '너비 × 높이 @ Hz (예: 1920x1080@59.94)', prompt: { model: '1920x1080@30', type: 'text' }, cancel: true }).onOk((value: string) => {
    const match = /^(\d+)\s*[x×]\s*(\d+)\s*[@/]\s*(\d+(?:\.\d+)?)$/.exec(value.trim())
    if (!match) { error.value = '너비x높이@Hz 형식으로 입력하세요.'; return }
    mode.value = { width: Number(match[1]), height: Number(match[2]), frameRate: Number(match[3]) }; void openVideo()
  })
}
function send(input: MiniInput): Promise<void> {
  const uid = selected.value, generation = inputGeneration
  if (!canInput.value) return Promise.resolve()
  return api.input(uid, input).catch(async e => {
    if (uid === selected.value && generation === inputGeneration) { error.value = String(e); await releaseInputs() }
  })
}
function keyDown(event: KeyboardEvent): void {
  if (!canInput.value) return
  if (specialKeys.value) return // Native capture suppresses the host event and owns DOWN/UP.
  const usage = miniKeyboardUsage(event)
  if (usage === undefined) return
  if (!specialKeys.value && isMiniSpecialKey(usage)) return
  event.preventDefault()
  if (!event.repeat && !heldKeys.has(event.code)) {
    const alreadyDown = [...heldKeys.values()].includes(usage)
    heldKeys.set(event.code, usage)
    if (!alreadyDown) void send({ kind: 'key', usage, down: true })
  }
}
function keyUp(event: KeyboardEvent): void {
  if (specialKeys.value) return
  const usage = heldKeys.get(event.code)
  if (usage !== undefined) {
    heldKeys.delete(event.code); event.preventDefault()
    if (![...heldKeys.values()].includes(usage)) void send({ kind: 'key', usage, down: false })
  }
}
function point(event: MouseEvent): { x: number; y: number } | undefined {
  const el = canvas.value, info = capture.value
  if (!el || !info?.width) return
  return absolutePoint(el.getBoundingClientRect(), info.width, info.height, event.clientX, event.clientY, Boolean(heldButtons.size))
}
function pointerMove(event: PointerEvent): void {
  const p = point(event); if (!canInput.value || !p) return
  pointer.move(p)
}
function pointerDown(event: PointerEvent): void {
  const p = point(event), button = [1, 3, 2, 4, 5][event.button]
  if (!canInput.value || !p || !button) return
  if (heldButtons.has(button)) return
  event.preventDefault(); stage.value?.focus(); stage.value?.setPointerCapture(event.pointerId); heldButtons.add(button)
  pointer.button(p, button, true)
}
function pointerUp(event: PointerEvent): void {
  const button = [1, 3, 2, 4, 5][event.button]
  if (button && heldButtons.has(button)) {
    const p = point(event)
    heldButtons.delete(button)
    pointer.button(p, button, false)
  }
}
function lostCapture(): void { if (heldButtons.size) void releaseInputs() }
function wheel(event: WheelEvent): void {
  if (!canInput.value || !point(event)) return
  const units = (d: number) => -Math.sign(d) * Math.min(127, Math.max(1, Math.round(Math.abs(d) / 100)))
  pointer.wheel(units(event.deltaY), units(event.deltaX))
}
function blur(): void { windowFocused.value = false; void releaseInputs() }
function focus(): void { windowFocused.value = !document.hidden; void syncKeyboard() }
function visibility(): void { if (document.hidden) blur(); else focus() }
watch(() => route.fullPath, async () => {
  specialKeys.value = false
  const ticket = ++selectionEpoch
  await stopVideo()
  if (disposed || ticket !== selectionEpoch) return
  if (controlMode.value) { selected.value = String(route.params.uid ?? ''); restoreMode(selected.value); if (active.value?.captureId) await openVideo() }
})
onBeforeRouteLeave(async () => { await stopVideo(); if (selected.value) await api.tools.deactivate(selected.value) })
onMounted(() => {
  unsubscribeKeyboard = api.keyboard.onState(value => {
    if (value.uid !== selected.value) return
    nativeKeyboard.value = value.active
    if (value.error) { specialKeys.value = false; error.value = value.error }
  })
  unsubscribe = api.onInventory(value => {
    state.value = value
    if (selected.value && !value.boards.some(b => b.channels.some(c => c.uid === selected.value && c.connected))) void stopVideo()
  })
  unsubscribeFrame = api.video.onFrame(frame => { void drawFrame(frame) })
  unsubscribeEnded = api.video.onEnded(value => { if (value.uid === selected.value) { error.value = value.message; void releaseInputs() } })
  window.addEventListener('blur', blur); window.addEventListener('focus', focus); document.addEventListener('visibilitychange', visibility)
  void rescan()
})
onBeforeUnmount(() => {
  disposed = true; ++epoch; unsubscribe(); unsubscribeFrame(); unsubscribeEnded(); unsubscribeKeyboard(); void releaseInputs(); void api.video.stop()
  window.removeEventListener('blur', blur); window.removeEventListener('focus', focus); document.removeEventListener('visibilitychange', visibility)
})
</script>

<style scoped lang="scss">
.mini-shell { background: #080d15; color: #e2eaf4; }
.mini-header { background: #0c1420; border-bottom: 1px solid #243247; }
.q-toolbar { min-height: 72px; gap: 12px; padding: 8px 24px; flex-wrap: wrap; }
.q-toolbar__title { min-width: 200px; }
.toolbar-video-controls { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
.video-mode { width: 200px; }
.q-toolbar__title strong { font-size: 20px; } .q-toolbar__title small { display: block; font-size: 9px; letter-spacing: 2px; color: #7f95b2; }
.mini-nav-heading { padding: 24px 20px 18px; font-size: 10px; letter-spacing: 2px; color: #7f95b2; }
.mini-board { padding: 10px 12px 18px; border-bottom: 1px solid #233047; }
.mini-board-title { display: flex; gap: 10px; align-items: center; padding: 8px; font-size: 13px; font-weight: 600; }
.mini-board-meta { padding: 0 8px 10px; font-size: 11px; color: #8193a9; }
.mini-selected { background: #15333e; border-radius: 8px; color: #79e4eb; }
.mini-content { padding: 30px; height: calc(100vh - 72px); min-height: 0 !important; overflow-y: auto; }
.mini-eyebrow { font-size: 10px; letter-spacing: 2px; color: #57d4df; }
h1 { font-size: 25px; line-height: 1.4; margin: 5px 0; font-weight: 600; }
.mini-stage { position: relative; height: calc(100vh - 430px); min-height: 280px; background: #03060a; border: 1px solid #283750; border-radius: 12px; overflow: hidden; outline: none; }
.mini-stage:focus { border-color: #4bd8e3; }
.mini-video { width: 100%; height: 100%; object-fit: contain; }
.mini-placeholder { display: flex; flex-direction: column; align-items: center; justify-content: center; height: 100%; color: #64778e; }
.mini-placeholder h2 { font-size: 17px; line-height: 1.5; margin: 20px 0 5px; color: #b2bfd0; }
.mini-placeholder p { font-size: 12px; }
.mini-notice { background: #142637; color: #a8c6df; font-size: 12px; }
.channel-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(290px, 1fr)); gap: 20px; margin-top: 30px; }
.channel-card { background: #101c2b; border-color: #2b3c53; cursor: pointer; h2 { font-size: 22px; margin: 18px 0 8px; } }
.selected-card { border-color: #39c9d1; } .mono { font-family: Consolas, monospace; }
.console-toolbar { display: flex; align-items: center; gap: 5px; padding: 0 18px 6px; flex-wrap: wrap; }
.console-page { position: relative; overflow: hidden; }
.console-page .mini-notice { padding: 5px 12px; min-height: 24px; margin-top: 5px; }
.control-shell :deep(.q-page-container) { height: 100vh; display: flex; flex-direction: column; box-sizing: border-box; }
.control-shell .console-page { flex: 1; height: auto; padding: 0; }
.control-shell .mini-header { background: #0c1420e8; }
.control-shell .mini-stage { position: absolute; inset: 0; height: 100%; margin: 0; border: 0; border-radius: 0; }
.control-shell .mini-notice, .control-shell .bg-red-10 { position: fixed; bottom: 34px; left: 18px; right: 18px; z-index: 2; }
@media (max-width: 1050px) { .mini-content { padding: 20px; } }
</style>
