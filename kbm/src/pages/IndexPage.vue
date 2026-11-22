<template>
  <q-layout view="hHh lpR fFf" class="app-shell">
    <q-header class="app-header">
      <q-toolbar class="toolbar">
        <div class="brand-block">
          <div class="brand-mark">K</div>
          <div>
            <div class="brand-title">KBM CONTROL</div>
            <div class="brand-subtitle">DUAL CAPTURE CONSOLE</div>
          </div>
        </div>

        <q-space />

        <div class="status-cluster">
          <div class="port-state" :class="{ online: masterState.online[0] }">
            <span class="state-dot" />
            UART 0
            <strong>{{ masterState.online[0] ? 'ONLINE' : 'OFFLINE' }}</strong>
            <q-tooltip>{{ diagnosticText(0) }}</q-tooltip>
          </div>
          <div class="port-state" :class="{ online: masterState.online[1] }">
            <span class="state-dot" />
            UART 1
            <strong>{{ masterState.online[1] ? 'ONLINE' : 'OFFLINE' }}</strong>
            <q-tooltip>{{ diagnosticText(1) }}</q-tooltip>
          </div>
        </div>

        <div class="screen-routing">
          <q-select v-for="(port, screen) in screenPorts" :key="screen"
            :model-value="port" :options="uartOptions" emit-value map-options
            dense outlined dark :label="`화면 ${screen + 1}`"
            :aria-label="`화면 ${screen + 1} UART 포트`"
            @update:model-value="changeScreenPort(screen, $event)" />
        </div>

        <q-btn
          unelevated
          no-caps
          :color="masterState.connected ? 'grey-9' : 'cyan-8'"
          :icon="masterState.connected ? 'usb' : 'usb_off'"
          :label="masterState.connected ? 'MASTER 연결됨' : 'MASTER 자동 연결'"
          @click="toggleMasterConnection"
        />
      </q-toolbar>

      <div class="tab-row">
        <q-tabs
          v-model="activeTab"
          dense
          align="left"
          active-color="cyan-4"
          indicator-color="cyan-4"
          class="screen-tabs"
        >
          <q-tab name="screen-1" no-caps>
            <span class="tab-label">화면 1</span>
            <q-badge outline color="cyan-6">UART {{ screenPorts[0] }}</q-badge>
            <span class="camera-dot" :class="{ active: Boolean(captures[0]) }" />
          </q-tab>
          <q-tab name="screen-2" no-caps>
            <span class="tab-label">화면 2</span>
            <q-badge outline color="cyan-6">UART {{ screenPorts[1] }}</q-badge>
            <span class="camera-dot" :class="{ active: Boolean(captures[1]) }" />
          </q-tab>
        </q-tabs>

        <div class="header-tools">
          <q-btn flat no-caps icon="terminal" label="시리얼 중계" aria-label="시리얼 중계" @click="serialDialogOpen = true" />
          <NetworkDialogs :connected="masterState.connected && masterState.protocolVersion === 3" />
          <DiskShareDialog :connected="masterState.connected && masterState.protocolVersion === 3" />
          <UartDiagnostics :connected="masterState.connected && masterState.protocolVersion === 3" @testing="handleLinkTesting" />
          <OperationLogs />
          <HidDiagnostics />
          <WakePcDialog />
          <SpecialKeysDialog :active-port="activeTarget" @prepare="releaseInputs" />
        </div>

        <q-space />

        <CaptureSettingsDialog :inventory="videoInventory" :assignments="captureAssignments"
          :captures="captures" :uart-ports="screenPorts" :busy="captureLoading" @prepare="releaseInputs"
          @assign="assignCaptureDevices" @mode="changeCaptureMode" />
        <q-btn
          flat dense round icon="refresh" aria-label="캡처 장치 다시 검색"
          :loading="captureLoading" @click="refreshCaptures"
        >
          <q-tooltip>캡처 장치 다시 검색</q-tooltip>
        </q-btn>
        <q-btn
          flat dense round icon="fullscreen" aria-label="영상 전체화면"
          @click="enterFullscreen"
        >
          <q-tooltip>영상 전체화면</q-tooltip>
        </q-btn>
        <q-btn
          flat dense round icon="tune" aria-label="캡처 장치 블랙리스트 설정"
          @click="settingsOpen = true"
        >
          <q-tooltip>캡처 장치 블랙리스트</q-tooltip>
        </q-btn>
      </div>
    </q-header>

    <q-page-container>
      <q-page class="viewer-page">
        <section ref="viewerStage" class="viewer-stage">
          <video
            ref="videoOne"
            v-show="activeTab === 'screen-1' && captures[0]"
            class="capture-video"
            autoplay muted playsinline
          />
          <video
            ref="videoTwo"
            v-show="activeTab === 'screen-2' && captures[1]"
            class="capture-video"
            autoplay muted playsinline
          />

          <div v-if="!activeCapture" class="empty-state">
            <q-icon name="videocam_off" size="64px" />
            <div class="empty-title">화면 {{ activeIndex + 1 }} 신호 없음</div>
            <div class="empty-copy">USB 캡처 보드를 연결하거나 장치를 다시 검색하세요.</div>
            <q-btn outline no-caps color="cyan-4" label="장치 다시 검색"
              :loading="captureLoading" @click="refreshCaptures" />
          </div>

          <div v-if="activeCapture" class="video-meta">
            <span>화면 {{ activeIndex + 1 }}</span>
            <strong>{{ activeCapture.width }} × {{ activeCapture.height }}</strong>
            <span>{{ formatFrameRate(activeCapture.frameRate) }} FPS</span>
            <span class="device-name">{{ activeCapture.label }}</span>
            <span v-if="activeCapture.usbId" class="device-usb">{{ activeCapture.usbId }}</span>
          </div>

          <div
            v-if="activeCapture"
            class="input-surface"
            tabindex="0"
            @contextmenu.prevent
            @pointerdown="handlePointerDown"
            @pointermove="handlePointerMove"
            @pointerup="handlePointerUp"
            @pointercancel="handlePointerCancel"
            @wheel.prevent="handleWheel"
            @keydown="handleKeyDown"
            @keyup="handleKeyUp"
            @blur="releaseInputs"
          >
            <div v-if="!masterState.connected || masterState.protocolVersion !== 3 || testingPort === activeTarget" class="input-lock">
              {{ testingPort === activeTarget ? 'UART 속도 시험 중 · 입력 차단' : 'MASTER 연결 후 입력 제어 가능' }}
            </div>
          </div>
          <VirtualKeyboard :visible="keyboardOpen" :port="activeTarget" :screen="activeIndex + 1" @close="keyboardOpen = false" @prepare="releaseInputs" />
        </section>

        <div class="footer-strip">
          <span>화면 {{ activeIndex + 1 }} → UART {{ activeTarget }}</span>
          <span class="separator" />
          <span>마우스 ABS 0–32767</span>
          <span class="separator" />
          <span>오디오 비활성</span>
          <q-space />
          <span v-if="masterState.error" class="error-text">{{ masterState.error }}</span>
          <span v-else>STX · OP · LEN · PAYLOAD · ETX · CHK</span>
          <q-btn v-if="!keyboardOpen" flat dense no-caps icon="keyboard" label="가상 키보드 열기" @click="releaseInputs(); keyboardOpen = true" />
        </div>
      </q-page>
    </q-page-container>

    <q-dialog v-model="serialDialogOpen">
      <q-card class="serial-console-card">
        <q-card-section class="settings-heading">
          <div class="text-h6">시리얼 중계</div>
          <q-space />
          <q-btn v-close-popup flat round dense icon="close" aria-label="시리얼 중계 닫기" />
        </q-card-section>
        <q-card-section>
          <q-tabs v-model="serialPort" dense align="left" active-color="cyan-4">
            <q-tab :name="0" label="UART 0" />
            <q-tab :name="1" label="UART 1" />
          </q-tabs>
          <div class="serial-signals">
            <q-badge :color="serialFlags & 1 ? 'teal-7' : 'grey-8'">USB {{ serialFlags & 1 ? '연결' : '미연결' }}</q-badge>
            <q-badge :color="serialFlags & 2 ? 'teal-7' : 'grey-8'">DTR {{ serialFlags & 2 ? 'ON' : 'OFF' }}</q-badge>
            <q-badge :color="serialFlags & 4 ? 'teal-7' : 'grey-8'">RTS {{ serialFlags & 4 ? 'ON' : 'OFF' }}</q-badge>
            <q-badge :color="serialReady ? 'teal-7' : 'orange-9'">{{ serialReady ? '송신 준비' : 'slave COM 열기 대기' }}</q-badge>
            <q-space />
            <q-toggle v-model="serialHex" dense label="HEX 수신" />
          </div>
          <q-input :model-value="serialLogs[serialPort]" type="textarea" readonly outlined dark
            class="serial-log" label="수신 데이터" :input-style="{ minHeight: '260px', fontFamily: 'monospace' }" />
          <div class="serial-send-row">
            <q-input v-model="serialDraft" dense outlined dark label="전송할 텍스트"
              @keyup.enter="sendSerialText" />
            <q-checkbox v-model="serialNewline" dense label="LF 추가" />
            <q-btn unelevated no-caps color="cyan-8" label="전송" :loading="serialSending"
              :disable="!masterState.connected || !serialReady || !serialDraft" @click="sendSerialText" />
          </div>
          <div class="text-caption text-grey-5 q-mt-sm">
            slave USB COM에서 DTR/RTS를 켜면 송신할 수 있습니다. 주 제어 연결로 양방향 중계합니다.
          </div>
        </q-card-section>
      </q-card>
    </q-dialog>

    <q-dialog v-model="settingsOpen">
      <q-card class="device-settings-card">
        <q-card-section class="settings-heading">
          <div>
            <div class="text-h6">영상 장치 블랙리스트</div>
            <div class="text-caption text-grey-5">목록에 없는 영상 입력만 화면 1·2에 배정합니다.</div>
          </div>
          <q-space />
          <q-btn v-close-popup flat round dense icon="close" aria-label="닫기" />
        </q-card-section>

        <q-separator dark />
        <q-card-section class="device-list-section">
          <div class="settings-label">현재 인식 장치</div>
          <q-list bordered separator dark class="device-list">
            <q-item v-for="device in videoInventory" :key="device.deviceId">
              <q-item-section>
                <q-item-label>{{ device.label }}</q-item-label>
                <q-item-label caption>
                  {{ device.usbId || 'USB ID 없음' }} · {{ device.pnp?.manufacturer || '제조사 미상' }}
                </q-item-label>
                <q-item-label v-if="device.pnp?.instanceId" caption class="instance-id">
                  {{ device.pnp.instanceId }}
                </q-item-label>
              </q-item-section>
              <q-item-section side>
                <q-badge :color="device.blacklisted ? 'red-8' : 'teal-7'">
                  {{ device.blacklisted ? '제외됨' : '사용 가능' }}
                </q-badge>
                <q-btn
                  class="q-mt-xs" flat dense no-caps
                  :color="device.blacklisted ? 'cyan-4' : 'red-4'"
                  :label="device.blacklisted ? '허용' : '제외'"
                  @click="toggleDeviceBlacklist(device)"
                />
              </q-item-section>
            </q-item>
            <q-item v-if="videoInventory.length === 0">
              <q-item-section class="text-grey-6">인식된 영상 장치가 없습니다.</q-item-section>
            </q-item>
          </q-list>
        </q-card-section>

        <q-card-section>
          <div class="settings-label">이름 포함 문자열</div>
          <div class="blacklist-add-row">
            <q-input v-model="newBlacklistName" dense outlined dark label="예: integrated camera"
              @keyup.enter="addBlacklistName" />
            <q-btn outline no-caps color="cyan-4" label="추가" @click="addBlacklistName" />
          </div>
          <div class="chip-row">
            <q-chip v-for="name in captureBlacklist.names" :key="name" removable dark
              color="blue-grey-9" text-color="grey-2" @remove="removeBlacklistName(name)">
              {{ name }}
            </q-chip>
          </div>

          <div class="settings-label q-mt-md">USB VID:PID</div>
          <div class="blacklist-add-row">
            <q-input v-model="newBlacklistUsb" dense outlined dark label="예: 045e:0c1e"
              @keyup.enter="addBlacklistUsb" />
            <q-btn outline no-caps color="cyan-4" label="추가" @click="addBlacklistUsb" />
          </div>
          <div class="chip-row">
            <q-chip v-for="usb in captureBlacklist.usbIds" :key="usb" removable dark
              color="blue-grey-9" text-color="grey-2" @remove="removeBlacklistUsb(usb)">
              {{ usb }}
            </q-chip>
          </div>
        </q-card-section>

        <q-card-actions align="between">
          <q-btn flat no-caps color="grey-5" label="기본값 복원" @click="restoreDefaultBlacklist" />
          <q-btn v-close-popup unelevated no-caps color="cyan-8" label="완료" />
        </q-card-actions>
      </q-card>
    </q-dialog>
  </q-layout>
</template>

<script setup lang="ts">
import NetworkDialogs from '../components/NetworkDialogs.vue'
import DiskShareDialog from '../components/DiskShareDialog.vue'
import UartDiagnostics from '../components/UartDiagnostics.vue'
import OperationLogs from '../components/OperationLogs.vue'
import HidDiagnostics from '../components/HidDiagnostics.vue'
import WakePcDialog from '../components/WakePcDialog.vue'
import SpecialKeysDialog from '../components/SpecialKeysDialog.vue'
import VirtualKeyboard from '../components/VirtualKeyboard.vue'
import CaptureSettingsDialog from '../components/CaptureSettingsDialog.vue'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'
import { useQuasar } from 'quasar'
import {
  defaultCaptureBlacklist,
  loadCaptureBlacklist,
  saveCaptureBlacklist,
  type CaptureBlacklist,
  openCaptureDevices,
  replaceCaptureMode,
  stopCaptureDevices,
  type CaptureSlot,
  type VideoDeviceInventory
} from '../services/capture-devices'
import {
  loadCaptureAssignments, saveCaptureAssignments, saveCaptureMode,
  loadCaptureUartPorts, initializeCaptureUartPorts, saveCaptureUartPort, captureScreenPorts,
  type CaptureAssignments, type CaptureMode
} from '../services/capture-preferences'
import { keyboardCodeToUsage, pointerButtonMask } from '../services/hid-keyboard'
import { masterSerial, type MasterSnapshot, type UartPort } from '../services/master-serial'

const $q = useQuasar()
const activeTab = ref<'screen-1' | 'screen-2'>('screen-1')
const captures = shallowRef<Array<CaptureSlot | undefined>>([undefined, undefined])
const videoInventory = shallowRef<VideoDeviceInventory[]>([])
const captureBlacklist = reactive<CaptureBlacklist>(loadCaptureBlacklist())
const captureLoading = ref(false)
const captureAssignments = ref<CaptureAssignments>(loadCaptureAssignments())
let captureDisposed = false
let captureRefreshPending = false
const settingsOpen = ref(false)
const keyboardOpen = ref(true)
const serialDialogOpen = ref(false)
const serialPort = ref<UartPort>(0)
const serialHex = ref(false)
const serialNewline = ref(true)
const serialDraft = ref('')
const serialSending = ref(false)
const serialLogs = ref<[string, string]>(['', ''])
const serialDecoders = [new TextDecoder(), new TextDecoder()]
const newBlacklistName = ref('')
const newBlacklistUsb = ref('')
const videoOne = ref<HTMLVideoElement>()
const videoTwo = ref<HTMLVideoElement>()
const viewerStage = ref<HTMLElement>()
const SCREEN_PORTS_KEY = 'kbm.screen-uart-ports.v1'
function loadScreenPorts(): [UartPort, UartPort] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(SCREEN_PORTS_KEY) ?? '')
    if (Array.isArray(saved) && saved.length === 2 && saved.every(port => port === 0 || port === 1)) {
      return saved as [UartPort, UartPort]
    }
  } catch { /* First run or invalid saved settings: use one port per screen. */ }
  return [0, 1]
}
const fallbackScreenPorts = ref<[UartPort, UartPort]>(loadScreenPorts())
const captureUartPorts = ref(loadCaptureUartPorts())
const screenPorts = computed(() => captureScreenPorts(
  captureAssignments.value, captureUartPorts.value, fallbackScreenPorts.value
))
const testingPort = ref<UartPort>()
function handleLinkTesting(port: UartPort | undefined): void {
  releaseInputs()
  testingPort.value = port
}
const uartOptions = [{ label: 'UART 0', value: 0 }, { label: 'UART 1', value: 1 }]
const masterState = reactive<MasterSnapshot>({
  connected: false,
  protocolVersion: 0,
  serialFlags: [0, 0],
  online: [false, false],
  diagnostics: [
    { txFrames: 0, rxBytes: 0, rxFrames: 0, decodeErrors: 0 },
    { txFrames: 0, rxBytes: 0, rxFrames: 0, decodeErrors: 0 }
  ],
  error: ''
})
// Remember where each DOWN went; its UP must go to that same port.
const pressedKeys = new Map<number, UartPort>()
const pressedButtons = new Map<number, UartPort>()
let unsubscribeMaster: (() => void) | undefined
let unsubscribeSerial: (() => void) | undefined
let unsubscribeBoot: (() => void) | undefined
let deviceRefreshTimer: ReturnType<typeof setTimeout> | undefined
let moveAnimationFrame = 0
let latestMove: { port: UartPort; x: number; y: number } | undefined
let lastInputErrorAt = 0

const activeIndex = computed(() => activeTab.value === 'screen-1' ? 0 : 1)
const activeTarget = computed(() => screenPorts.value[activeIndex.value] ?? 0)
const serialFlags = computed(() => masterState.serialFlags[serialPort.value])
const serialReady = computed(() => (serialFlags.value & 7) === 7)
const activeCapture = computed(() => captures.value[activeIndex.value])
const activeVideo = computed(() => activeIndex.value === 0 ? videoOne.value : videoTwo.value)

watch(captures, async () => {
  await nextTick()
  if (videoOne.value && videoOne.value.srcObject !== (captures.value[0]?.stream ?? null)) {
    videoOne.value.srcObject = captures.value[0]?.stream ?? null
  }
  if (videoTwo.value && videoTwo.value.srcObject !== (captures.value[1]?.stream ?? null)) {
    videoTwo.value.srcObject = captures.value[1]?.stream ?? null
  }
})

watch(activeTab, () => releaseInputs())

async function sendSerialText(): Promise<void> {
  if (serialSending.value || !serialDraft.value) return
  const port = serialPort.value
  const text = serialDraft.value + (serialNewline.value ? '\n' : '')
  serialSending.value = true
  try {
    await masterSerial.serialWrite(port, new TextEncoder().encode(text))
    serialDraft.value = ''
  } catch (error) {
    notifyError(messageOf(error))
  } finally {
    serialSending.value = false
  }
}

async function refreshCaptures(): Promise<void> {
  if (captureDisposed) return
  if (captureLoading.value) { captureRefreshPending = true; return }
  captureLoading.value = true
  releaseInputs()
  stopCaptureDevices(captures.value)
  captures.value = [undefined, undefined]
  await nextTick()
  try {
    const next = await openCaptureDevices(captureBlacklist, captureAssignments.value)
    if (captureDisposed) { stopCaptureDevices(next.slots); return }
    try {
      const ports = initializeCaptureUartPorts(next.assignments, screenPorts.value)
      saveCaptureAssignments(next.assignments)
      captureUartPorts.value = ports
    } catch (error) { stopCaptureDevices(next.slots); throw error }
    captures.value = next.slots
    videoInventory.value = next.inventory
    captureAssignments.value = next.assignments
    if (next.errors.length > 0) notifyError(`캡처 장치 열기 실패: ${next.errors.join(' / ')}`)
  } catch (error) {
    notifyError(`캡처 장치: ${messageOf(error)}`)
  } finally {
    captureLoading.value = false
    finishCaptureOperation()
  }
}

function finishCaptureOperation(): void {
  if (captureRefreshPending && !captureDisposed) {
    captureRefreshPending = false
    void refreshCaptures()
  }
}

async function assignCaptureDevices(assignments: CaptureAssignments): Promise<void> {
  if (captureLoading.value) return
  try {
    releaseInputs()
    // Pin the currently displayed boards before moving them, including migration
    // from the legacy screen-only settings. New boards inherit the slot default.
    initializeCaptureUartPorts(captureAssignments.value, screenPorts.value)
    const ports = initializeCaptureUartPorts(assignments, screenPorts.value)
    saveCaptureAssignments(assignments)
    captureUartPorts.value = ports
    captureAssignments.value = [...assignments]
    await refreshCaptures()
  } catch (error) { notifyError(messageOf(error)) }
}

async function changeCaptureMode(index: 0 | 1, mode: CaptureMode): Promise<void> {
  if (captureLoading.value || captureDisposed) return
  const device = videoInventory.value.find(device => !device.blacklisted && device.deviceId === captureAssignments.value[index])
  if (!device) return
  captureLoading.value = true
  releaseInputs()
  const previous = captures.value[index]
  const next = [...captures.value]
  next[index] = undefined
  captures.value = next
  await nextTick()
  try {
    const result = await replaceCaptureMode(device, previous, mode)
    if (captureDisposed) { stopCaptureDevices([result.slot]); return }
    const slots = [...captures.value]
    slots[index] = result.slot
    captures.value = slots
    if (!result.applied) {
      window.kbmNative.logs.record('warn', 'ui.capture.mode_failed', { screen: index + 1, restored: Boolean(result.slot) })
      notifyError(`화면 ${index + 1} 모드 적용 실패: ${result.error}`)
      return
    }
    saveCaptureMode(device.deviceId, mode)
    window.kbmNative.logs.record('info', 'ui.capture.mode_changed', {
      screen: index + 1, automatic: mode.kind === 'auto', width: result.slot?.width ?? 0,
      height: result.slot?.height ?? 0, frameRate: result.slot?.frameRate ?? 0
    })
  } catch (error) { notifyError(`영상 설정 저장/적용: ${messageOf(error)}`) }
  finally { captureLoading.value = false; finishCaptureOperation() }
}

async function commitBlacklist(next: CaptureBlacklist): Promise<void> {
  const saved = saveCaptureBlacklist(next)
  captureBlacklist.names.splice(0, captureBlacklist.names.length, ...saved.names)
  captureBlacklist.usbIds.splice(0, captureBlacklist.usbIds.length, ...saved.usbIds)
  await refreshCaptures()
}

function addBlacklistName(): void {
  const value = newBlacklistName.value.trim().toLowerCase()
  if (!value) return
  newBlacklistName.value = ''
  void commitBlacklist({ ...captureBlacklist, names: [...captureBlacklist.names, value] })
}

function addBlacklistUsb(): void {
  const value = newBlacklistUsb.value.trim().toLowerCase()
  if (!/^[0-9a-f]{4}:?[0-9a-f]{4}$/.test(value)) {
    notifyError('USB ID는 VID:PID 형식으로 입력하세요. 예: 045e:0c1e')
    return
  }
  newBlacklistUsb.value = ''
  void commitBlacklist({ ...captureBlacklist, usbIds: [...captureBlacklist.usbIds, value] })
}

function removeBlacklistName(name: string): void {
  void commitBlacklist({ ...captureBlacklist, names: captureBlacklist.names.filter(item => item !== name) })
}

function removeBlacklistUsb(usb: string): void {
  void commitBlacklist({ ...captureBlacklist, usbIds: captureBlacklist.usbIds.filter(item => item !== usb) })
}

function toggleDeviceBlacklist(device: VideoDeviceInventory): void {
  if (device.blacklisted) {
    const haystack = `${device.label} ${device.pnp?.name ?? ''}`.toLowerCase()
    void commitBlacklist({
      names: captureBlacklist.names.filter(name => !haystack.includes(name)),
      usbIds: captureBlacklist.usbIds.filter(usb => usb !== device.usbId)
    })
    return
  }
  if (device.usbId) {
    void commitBlacklist({ ...captureBlacklist, usbIds: [...captureBlacklist.usbIds, device.usbId] })
  } else {
    void commitBlacklist({ ...captureBlacklist, names: [...captureBlacklist.names, device.label] })
  }
}

function restoreDefaultBlacklist(): void {
  void commitBlacklist(defaultCaptureBlacklist())
}

async function toggleMasterConnection(): Promise<void> {
  if (masterState.connected) {
    await masterSerial.requestStatus().catch(error => notifyError(messageOf(error)))
    return
  }
  const connected = await masterSerial.connect()
  if (!connected) notifyError('VID 8857 / PID 1223 master를 찾지 못했습니다.')
}

function changeScreenPort(screen: number, value: unknown): void {
  if ((screen !== 0 && screen !== 1) || (value !== 0 && value !== 1)) return
  if (screenPorts.value[screen] === value) return
  if (screen === activeIndex.value) releaseInputs()
  try {
    const deviceId = captureAssignments.value[screen]
    if (deviceId) {
      captureUartPorts.value = saveCaptureUartPort(deviceId, value)
    } else {
      const fallback: [UartPort, UartPort] = [...fallbackScreenPorts.value]
      fallback[screen] = value
      localStorage.setItem(SCREEN_PORTS_KEY, JSON.stringify(fallback))
      fallbackScreenPorts.value = fallback
    }
  } catch (error) {
    notifyError(messageOf(error))
  }
}

function coordinatesFromPointer(event: PointerEvent | WheelEvent):
{ x: number; y: number; inside: boolean } | undefined {
  const video = activeVideo.value
  if (!video || video.videoWidth === 0 || video.videoHeight === 0) return
  const rect = video.getBoundingClientRect()
  const scale = Math.min(rect.width / video.videoWidth, rect.height / video.videoHeight)
  const contentWidth = video.videoWidth * scale
  const contentHeight = video.videoHeight * scale
  const left = rect.left + (rect.width - contentWidth) / 2
  const top = rect.top + (rect.height - contentHeight) / 2
  const normalizedX = (event.clientX - left) / contentWidth
  const normalizedY = (event.clientY - top) / contentHeight
  return {
    x: Math.round(Math.max(0, Math.min(1, normalizedX)) * 0x7fff),
    y: Math.round(Math.max(0, Math.min(1, normalizedY)) * 0x7fff),
    inside: normalizedX >= 0 && normalizedX <= 1 && normalizedY >= 0 && normalizedY <= 1
  }
}

function handlePointerMove(event: PointerEvent): void {
  if (testingPort.value === activeTarget.value) return
  const point = coordinatesFromPointer(event)
  if (!point || (!point.inside && pressedButtons.size === 0)) return
  latestMove = { ...point, port: activeTarget.value }
  if (moveAnimationFrame !== 0) return
  moveAnimationFrame = requestAnimationFrame(() => {
    moveAnimationFrame = 0
    const move = latestMove
    latestMove = undefined
    if (move && masterState.connected) sendInput(masterSerial.mouseMove(move.port, move.x, move.y))
  })
}

function handlePointerDown(event: PointerEvent): void {
  if (testingPort.value === activeTarget.value) return
  const point = coordinatesFromPointer(event)
  const mask = pointerButtonMask(event.button)
  if (!point?.inside || mask === undefined || !masterState.connected) return
  ;(event.currentTarget as HTMLElement).focus()
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  const port = activeTarget.value
  pressedButtons.set(mask, port)
  sendInput(masterSerial.mouseMove(port, point.x, point.y))
  sendInput(masterSerial.mouseDown(port, mask))
}

function handlePointerUp(event: PointerEvent): void {
  const point = coordinatesFromPointer(event)
  const mask = pointerButtonMask(event.button)
  if (mask === undefined) return
  const port = pressedButtons.get(mask)
  pressedButtons.delete(mask)
  if (port !== undefined && masterState.connected) {
    if (point) sendInput(masterSerial.mouseMove(port, point.x, point.y))
    sendInput(masterSerial.mouseUp(port, mask))
  }
}

function handlePointerCancel(event: PointerEvent): void {
  const mask = pointerButtonMask(event.button)
  const port = mask !== undefined ? pressedButtons.get(mask) : undefined
  if (mask !== undefined) pressedButtons.delete(mask)
  if (mask !== undefined && port !== undefined && masterState.connected) {
    sendInput(masterSerial.mouseUp(port, mask))
  }
}

function handleWheel(event: WheelEvent): void {
  if (testingPort.value === activeTarget.value) return
  const point = coordinatesFromPointer(event)
  if (!point?.inside || !masterState.connected) return
  const wheel = wheelUnits(event.deltaY)
  const pan = wheelUnits(event.deltaX)
  sendInput(masterSerial.mouseMove(activeTarget.value, point.x, point.y, wheel, pan))
}

function handleKeyDown(event: KeyboardEvent): void {
  if (testingPort.value === activeTarget.value) return
  const usage = keyboardCodeToUsage(event.code)
  if (usage === undefined || !masterState.connected) return
  event.preventDefault()
  if (event.repeat || pressedKeys.has(usage)) return
  const port = activeTarget.value
  pressedKeys.set(usage, port)
  sendInput(masterSerial.keyDown(port, usage))
}

function handleKeyUp(event: KeyboardEvent): void {
  const usage = keyboardCodeToUsage(event.code)
  if (usage === undefined) return
  event.preventDefault()
  const port = pressedKeys.get(usage)
  pressedKeys.delete(usage)
  if (port !== undefined && masterState.connected) {
    sendInput(masterSerial.keyUp(port, usage))
  }
}

function releaseInputs(): void {
  if (moveAnimationFrame) cancelAnimationFrame(moveAnimationFrame)
  moveAnimationFrame = 0
  latestMove = undefined
  const targets = new Set([...pressedKeys.values(), ...pressedButtons.values()])
  pressedKeys.clear()
  pressedButtons.clear()
  if (masterState.connected) for (const port of targets) sendInput(masterSerial.releaseAll(port))
}

function sendInput(operation: Promise<unknown>): void {
  void operation.catch(error => {
    const now = Date.now()
    if (now - lastInputErrorAt > 2000) {
      lastInputErrorAt = now
      notifyError(messageOf(error))
    }
  })
}

function wheelUnits(delta: number): number {
  if (delta === 0) return 0
  const magnitude = Math.max(1, Math.round(Math.abs(delta) / 100))
  return -Math.sign(delta) * Math.min(127, magnitude)
}

function formatFrameRate(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(1)
}

function diagnosticText(port: 0 | 1): string {
  const value = masterState.diagnostics[port]
  return `TX frame ${value.txFrames} · RX byte ${value.rxBytes} · ` +
    `RX frame ${value.rxFrames} · decode error ${value.decodeErrors}`
}

async function enterFullscreen(): Promise<void> {
  await viewerStage.value?.requestFullscreen()
}

function scheduleDeviceRefresh(): void {
  if (deviceRefreshTimer) clearTimeout(deviceRefreshTimer)
  deviceRefreshTimer = setTimeout(() => void refreshCaptures(), 500)
}

function notifyError(message: string): void {
  $q.notify({ type: 'negative', message })
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

onMounted(() => {
  unsubscribeBoot = masterSerial.onSlaveBoot(port => {
    for (const [key, target] of pressedKeys) if (target === port) pressedKeys.delete(key)
    for (const [button, target] of pressedButtons) if (target === port) pressedButtons.delete(button)
    if (latestMove?.port === port) {
      if (moveAnimationFrame) cancelAnimationFrame(moveAnimationFrame)
      moveAnimationFrame = 0; latestMove = undefined
    }
  })
  unsubscribeSerial = masterSerial.onSerialData((port, bytes) => {
    const text = serialHex.value
      ? Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join(' ') + '\n'
      : serialDecoders[port]!.decode(bytes, { stream: true })
    serialLogs.value[port] = (serialLogs.value[port] + text).slice(-65_536)
  })
  unsubscribeMaster = masterSerial.subscribe(snapshot => {
    Object.assign(masterState, snapshot)
  })
  navigator.mediaDevices.addEventListener('devicechange', scheduleDeviceRefresh)
  window.addEventListener('blur', releaseInputs)
  void refreshCaptures()
  void masterSerial.initialize()
})

onBeforeUnmount(() => {
  captureDisposed = true
  unsubscribeSerial?.()
  unsubscribeBoot?.()
  unsubscribeMaster?.()
  navigator.mediaDevices.removeEventListener('devicechange', scheduleDeviceRefresh)
  window.removeEventListener('blur', releaseInputs)
  if (deviceRefreshTimer) clearTimeout(deviceRefreshTimer)
  if (moveAnimationFrame) cancelAnimationFrame(moveAnimationFrame)
  stopCaptureDevices(captures.value)
  releaseInputs()
})
</script>

