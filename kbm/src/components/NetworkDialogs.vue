<template>
  <q-btn flat no-caps icon="lan" label="네트워크" aria-label="네트워크" @click="networkOpen = true" />
  <q-btn flat no-caps icon="alt_route" label="포트 포워딩" aria-label="포트 포워딩" @click="forwardOpen = true" />
  <q-btn flat no-caps icon="usb" label="NCM 모드" @click="nativeOpen = true" />
  <q-dialog v-model="nativeOpen">
    <q-card class="network-card">
      <q-card-section class="row items-center"><div class="text-h6">Master 네이티브 CDC-NCM</div><q-space /><q-btn v-close-popup flat round icon="close" aria-label="NCM 모드 닫기" /></q-card-section>
      <q-card-section class="q-gutter-md">
        <div>OS 네트워크 어댑터 ↔ 두 slave를 펌웨어가 직접 중계합니다. 켜면 KBM Control의 DHCP·TCP/UDP·포워딩은 모두 중지됩니다.</div>
        <q-toggle v-model="nativeDraft" label="Master 네이티브 NCM 활성화" :disable="busy" />
        <q-toggle v-model="dhcpDraft" label="Master RP2040 DHCP 서버 활성화 (모드 전환 인자)" :disable="busy || !nativeDraft" />
        <div class="text-caption">DHCP 서버 식별자 및 Master PC IP: 192.168.127.1. Slave 기본 .100/.101, /24. Master PC는 무기한 임대입니다. DHCP UDP 67만 가로채며 .1의 다른 트래픽은 PC로 전달합니다. DHCP OFF일 때는 주소를 배포하지 않습니다.</div>
        <div class="text-orange-4">적용 시 Master USB와 두 Slave NCM을 껐다 켭니다. HID·시리얼도 잠시 끊깁니다. 인터넷 NAT/공유 기능은 제공하지 않습니다.</div>
        <q-badge :color="nativeState?.enabled ? 'teal-8' : 'grey-8'">{{ nativeKnown ? (nativeState?.enabled ? '네이티브 ON' : '네이티브 OFF') : '상태 미확인' }} · DHCP {{ nativeState?.enabled && nativeState?.dhcp ? 'ON' : 'OFF' }}</q-badge>
        <div class="text-caption">Master NCM: {{ !nativeKnown ? '미확인' : nativeState?.visible ? 'USB에 노출됨' : '숨김' }} · Slave 전환 대기: {{ transitionText }}</div>
        <div v-if="nativeKnown && !nativeState" class="text-orange-4">Master 펌웨어 업데이트가 필요합니다.</div>
        <q-btn color="cyan-8" label="모드 적용" :loading="busy" :disable="!connected || !nativeKnown || !nativeState || Boolean(nativeState.transition)" @click="applyNative" />
      </q-card-section>
    </q-card>
  </q-dialog>
  <q-dialog v-model="networkOpen">
    <q-card class="network-card">
      <q-card-section class="row items-center">
        <div class="text-h6">Slave 네트워크 · IPv4 / TCP / UDP / DHCP</div><q-space />
        <q-btn v-close-popup flat round icon="close" aria-label="네트워크 닫기" />
      </q-card-section>
      <q-card-section class="q-gutter-md">
        <div class="text-grey-5">독립된 두 /24 링크입니다. Master/게이트웨이는 {{ config.serverIp }}이며 slave 간 중계는 하지 않습니다.</div>
        <div v-for="port in ports" :key="port" class="row items-center q-gutter-md">
          <q-toggle :model-value="desired[port]" :label="`UART ${port} 네트워크`"
            :disable="busy || !connected || !online[port] || nativeOwned || !nativeKnown" @update:model-value="setEnabled(port, $event)" />
          <q-badge :color="online[port] ? 'teal-8' : 'grey-8'">{{ online[port] ? 'ONLINE' : 'OFFLINE' }}</q-badge>
          <q-badge :color="state.enabled[port] ? 'teal-8' : 'grey-8'">{{ state.enabled[port] ? 'ON' : 'OFF' }}</q-badge>
          <q-badge :color="linkStatus[port]?.ready ? 'teal-8' : 'grey-8'">USB NCM {{ linkStatus[port]?.ready ? '준비' : '대기' }}</q-badge>
          <span>RX {{ state.rxFrames[port] }} · TX {{ state.txFrames[port] }} · 큐 {{ state.queued[port] }} · 드롭 {{ state.dropped[port] }}</span>
          <span class="text-caption">{{ linkStatus[port]?.mac }}</span>
          <span class="text-caption">장치 {{ !linkStatus[port] ? '미확인' : linkStatus[port]?.enabled ? '노출 요청됨' : '숨김' }} · {{ linkStatus[port]?.advertisedBps ?? 0 }} bit/s 광고</span>
          <q-btn flat dense no-caps label="NCM 숨기기" :disable="busy || !connected || !online[port] || nativeOwned || !nativeKnown" @click="setEnabled(port, false)" />
        </div>
        <div v-if="nativeOwned" class="text-orange-4">네이티브 모드가 네트워크를 소유 중입니다. 앱 네트워크 기능은 OFF입니다.</div>
        <q-input v-model="config.serverIp" outlined dark label="Master / 기본 게이트웨이 (.1, /24)" :disable="anyEnabled" />
        <div class="row q-col-gutter-md">
          <q-input v-model.number="config.poolStart" class="col" type="number" outlined dark label="DHCP 시작 (마지막 옥텟)" :disable="anyEnabled" />
          <q-input v-model.number="config.poolEnd" class="col" type="number" outlined dark label="DHCP 끝" :disable="anyEnabled" />
          <q-input v-model.number="config.leaseSeconds" class="col" type="number" outlined dark label="임대 시간 (초)" :disable="anyEnabled" />
        </div>
        <q-btn outline no-caps label="설정 저장" :disable="anyEnabled || busy" @click="saveConfig" />
        <div v-for="lease in state.leases" :key="`${lease.port}:${lease.mac}`" class="text-caption">
          UART {{ lease.port }} · {{ lease.ip }} · {{ lease.mac }}
        </div>
        <div class="text-caption text-grey-5">네트워크는 실행할 때마다 OFF로 시작합니다. HID 제어 우선, 유한 큐·조각별 흐름 제어를 사용합니다.</div>
        <div v-if="state.error" class="text-negative">{{ state.error }}</div>
      </q-card-section>
    </q-card>
  </q-dialog>
  <q-dialog v-model="forwardOpen">
    <q-card class="network-card">
      <q-card-section class="row items-center">
        <div class="text-h6">로컬 입구 → Slave 포트 포워딩</div><q-space />
        <q-btn v-close-popup flat round icon="close" aria-label="포워딩 닫기" />
      </q-card-section>
      <q-card-section class="q-gutter-md">
        <div class="text-grey-5">새로운 127.0.0.1 수신 포트를 만들어 slave 서비스로 연결합니다. 기존 PC 서비스는 공유하지 않습니다.</div>
        <div v-for="rule in rules" :key="rule.id" class="forward-rule">
          <q-select v-model="rule.port" :options="portOptions" emit-value map-options dense outlined dark label="Slave UART" :disable="state.forwarding" />
          <q-select v-model="rule.protocol" :options="['tcp', 'udp']" dense outlined dark label="프로토콜" :disable="state.forwarding" />
          <q-input v-model.number="rule.listenPort" type="number" dense outlined dark label="127.0.0.1 포트" :disable="state.forwarding" />
          <q-icon name="arrow_forward" />
          <q-input v-model="rule.targetIp" dense outlined dark label="Slave IP" :disable="state.forwarding" />
          <q-input v-model.number="rule.targetPort" type="number" dense outlined dark label="Slave 포트" :disable="state.forwarding" />
          <q-btn flat round icon="delete_outline" :disable="state.forwarding" @click="rules = rules.filter(item => item.id !== rule.id)" aria-label="규칙 삭제" />
        </div>
        <div class="row q-gutter-sm">
          <q-btn outline no-caps icon="add" label="규칙 추가" :disable="state.forwarding || rules.length >= 32" @click="addRule" />
          <q-btn unelevated no-caps :color="state.forwarding ? 'orange-9' : 'cyan-8'"
            :label="state.forwarding ? '포워딩 끄기' : '규칙 저장하고 켜기'" :loading="busy" :disable="nativeOwned || !nativeKnown" @click="toggleForwarding" />
        </div>
        <div>연결 {{ state.connections }} · TCP/UDP 최대 32개 · UDP 유휴 연결 30초 만료</div>
        <div class="text-caption text-grey-5">예: 127.0.0.1:2222 → UART0 / 192.168.127.100:22 (SSH). 대상 UART 네트워크를 먼저 켜세요.</div>
        <div v-if="state.error" class="text-negative">{{ state.error }}</div>
      </q-card-section>
    </q-card>
  </q-dialog>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { useQuasar } from 'quasar'
import { masterSerial, type NativeModeState } from '../services/master-serial'
import { NetworkSession } from '../services/network-session'
import { defaultNetworkConfig, validateNetworkConfig, validateForwardRule,
  type ForwardRule, type NetworkConfig, type NetworkPort, type NetworkState } from '../shared/network'
const props = defineProps<{ connected: boolean }>()
const $q = useQuasar()
const ports: NetworkPort[] = [0, 1]
const portOptions = [{ label: 'UART 0', value: 0 }, { label: 'UART 1', value: 1 }]
const networkOpen = ref(false), forwardOpen = ref(false), busy = ref(false)
const nativeOpen = ref(false), nativeDraft = ref(false), dhcpDraft = ref(true), nativeKnown = ref(false)
const nativeState = ref<NativeModeState>()
const nativeOwned = computed(() => Boolean(nativeState.value?.enabled || nativeState.value?.visible || nativeState.value?.transition))
const transitionText = computed(() => ports.filter(port => (nativeState.value?.transition ?? 0) & (1 << port)).map(port => `UART ${port}`).join(', ') || '없음')
watch(nativeOpen, open => {
  if (open) { nativeDraft.value = nativeState.value?.enabled ?? false; dhcpDraft.value = nativeState.value?.dhcp ?? true }
})
const desired = ref<[boolean, boolean]>([false, false])
const online = ref<readonly [boolean, boolean]>([false, false])
const session = new NetworkSession((port, enabled) => masterSerial.networkEnable(port, enabled),
  (port, enabled) => window.kbmNative.network.enable(port, enabled))
const config = reactive<NetworkConfig>({ ...defaultNetworkConfig })
const rules = ref<ForwardRule[]>([])
const state = reactive<NetworkState>({ enabled: [false, false], forwarding: false,
  rxFrames: [0, 0], txFrames: [0, 0], dropped: [0, 0], queued: [0, 0], leases: [], connections: 0, error: '' })
const anyEnabled = computed(() => state.enabled.some(Boolean))
const linkStatus = ref<Array<{ ready: boolean; enabled: boolean; mac: string; advertisedBps: number } | undefined>>([])
let timer: ReturnType<typeof setInterval> | undefined
let unsubscribeMaster: (() => void) | undefined
let pollBusy = false
function notify(error: unknown): void { $q.notify({ type: 'negative', message: error instanceof Error ? error.message : String(error) }) }
async function poll(): Promise<void> {
  if (pollBusy) return
  pollBusy = true
  try {
    Object.assign(state, await window.kbmNative.network.state())
    if (props.connected) {
      nativeState.value = await masterSerial.queryNativeMode()
      nativeKnown.value = true
      if (nativeOwned.value) {
        await session.suspend(); desired.value = [false, false]
        await window.kbmNative.network.stopForward()
      } else session.suspended = false
    }
    await Promise.all(ports.map(async port => {
      session.reportLocal(port, state.enabled[port])
      const available = props.connected && online.value[port] && !masterSerial.isLinkTesting(port) && !masterSerial.isRebooting(port)
      if (!available) {
        linkStatus.value[port] = undefined
        await session.observe(port, { available: false, requested: false, ready: false }); return
      }
      try {
        const status = await masterSerial.networkStatus(port)
        linkStatus.value[port] = status
        await session.observe(port, { available: !nativeOwned.value, requested: status.enabled, ready: status.ready })
      } catch {
        linkStatus.value[port] = undefined
        await session.observe(port, { available: false, requested: false, ready: false })
      }
    }))
    desired.value = [...session.desired]
  }
  catch (error) { nativeKnown.value = false; notify(error) } finally { pollBusy = false }
}
async function saveConfig(): Promise<void> {
  try {
    validateNetworkConfig(config)
    await window.kbmNative.network.configure({ ...config })
    localStorage.setItem('kbm.network.config.v1', JSON.stringify(config))
  } catch (error) { notify(error); throw error }
}
async function setEnabled(port: NetworkPort, enabled: boolean): Promise<void> {
  busy.value = true
  try {
    if (enabled) {
      if (nativeOwned.value) throw new Error('네이티브 모드에서는 앱 네트워크를 켤 수 없습니다.')
      if (!anyEnabled.value) await saveConfig()
    }
    await session.setDesired(port, enabled)
    desired.value = [...session.desired]
    await poll()
  } catch (error) { notify(error) } finally { busy.value = false }
}
async function applyNative(): Promise<void> {
  busy.value = true
  try {
    await session.suspend(); desired.value = [false, false]
    await window.kbmNative.network.stopForward()
    // Stop packet-producing stacks before the firmware takes ownership.
    await Promise.all(ports.map(port => window.kbmNative.network.enable(port, false)))
    nativeState.value = await masterSerial.setNativeMode(nativeDraft.value, dhcpDraft.value)
    $q.notify({ type: 'info', message: '모드 명령을 수락했습니다. USB와 Slave NCM 재연결을 기다립니다.' })
  } catch (error) { notify(error) } finally { busy.value = false }
}
function addRule(): void {
  rules.value.push({ id: crypto.randomUUID(), port: 0, protocol: 'tcp', listenPort: 2222 + rules.value.length,
    targetIp: config.serverIp.slice(0, config.serverIp.lastIndexOf('.') + 1) + config.poolStart, targetPort: 22 })
}
async function toggleForwarding(): Promise<void> {
  busy.value = true
  try {
    if (state.forwarding) await window.kbmNative.network.stopForward()
    else {
      if (nativeOwned.value) throw new Error('네이티브 모드에서는 앱 포워딩을 켤 수 없습니다.')
      for (const rule of rules.value) validateForwardRule(rule, config)
      await window.kbmNative.network.forward(rules.value.map(rule => ({ ...rule })))
      localStorage.setItem('kbm.network.forwards.v1', JSON.stringify(rules.value))
    }
    await poll()
  } catch (error) { notify(error) } finally { busy.value = false }
}
onMounted(() => {
  let previousOnline: readonly [boolean, boolean] = [false, false]
  let previousFlags: readonly [number, number] = [0, 0]
  unsubscribeMaster = masterSerial.subscribe(snapshot => {
    online.value = snapshot.online
    if (!snapshot.connected) nativeKnown.value = false
    for (const port of ports) {
      const lostUart = previousOnline[port] && !snapshot.online[port]
      const lostUsb = Boolean(previousFlags[port] & 1) && !(snapshot.serialFlags[port] & 1)
      if (!snapshot.connected || lostUart || lostUsb)
        void session.observe(port, { available: false, requested: false, ready: false }).catch(notify)
    }
    previousOnline = snapshot.online; previousFlags = snapshot.serialFlags
  })
  try {
    const saved = JSON.parse(localStorage.getItem('kbm.network.config.v1') ?? 'null') as NetworkConfig | null
    if (saved) { validateNetworkConfig(saved); Object.assign(config, saved) }
    const savedRules = JSON.parse(localStorage.getItem('kbm.network.forwards.v1') ?? '[]') as ForwardRule[]
    if (Array.isArray(savedRules) && savedRules.length <= 32) {
      for (const rule of savedRules) validateForwardRule(rule, config)
      rules.value = savedRules
    }
  } catch { /* Invalid persisted settings are not activated. */ }
  void window.kbmNative.network.configure({ ...config }).catch(notify)
  void poll(); timer = setInterval(() => void poll(), 1000)
})
onBeforeUnmount(() => { unsubscribeMaster?.(); if (timer) clearInterval(timer) })
</script>
<style scoped lang="scss">
.network-card { width: 1000px; max-width: 96vw; background: #101b29; }
.forward-rule { display: grid; grid-template-columns: 110px 90px 140px 24px minmax(140px,1fr) 110px 40px; gap: 8px; align-items: center; }
@media(max-width:800px) { .forward-rule { grid-template-columns: repeat(3,1fr); } }
</style>
