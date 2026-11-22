<template>
  <q-btn flat no-caps icon="power_settings_new" label="PC 깨우기" @click="open = true" />
  <q-dialog v-model="open" :persistent="busy">
    <q-card style="width:800px;max-width:95vw;background:#101b29">
      <q-card-section class="row items-center"><div class="text-h6">PC 깨우기</div><q-space /><q-btn v-close-popup flat round icon="close" :disable="busy" /></q-card-section>
      <q-card-section class="q-gutter-md">
        <div>USB는 절전 해제를 요청합니다. 종료된 PC는 실제 유선 LAN의 WOL 지원·BIOS 설정·대기 전원이 필요합니다.</div>
        <div class="text-caption">지원하지 않는 경우 오류 없이 무시하며 결과 코드로 구분합니다. 신호/패킷 발신은 PC 기상 완료를 뜻하지 않습니다. 자동 발신·재시도는 하지 않습니다.</div>
        <div v-for="port in [0, 1] as const" :key="port" class="q-pa-md" style="border:1px solid #304050">
          <div class="text-subtitle1">UART {{ port }} PC</div>
          <div class="row q-gutter-sm q-my-sm">
            <q-btn outline no-caps label="USB 절전 해제" :disable="busy || !usbAvailable(port)" @click="usbWake(port)" />
            <q-btn unelevated no-caps color="cyan-8" label="WOL 전원 켜기" :disable="busy || !settings[port].enabled" @click="wolWake(port)" />
          </div>
          <q-toggle v-model="settings[port].enabled" label="이 PC의 WOL 사용" :disable="busy" />
          <q-input v-model="settings[port].mac" outlined dark label="대상 PC의 실제 유선 LAN MAC (NCM MAC 아님)" placeholder="00:11:22:33:44:55" :disable="busy" />
          <q-select v-model="settings[port].localAddress" :options="adapterOptions" emit-value map-options outlined dark label="이 PC에서 WOL을 보낼 실제 LAN 어댑터" :disable="busy" class="q-mt-sm" />
          <div class="text-caption q-mt-sm" aria-live="polite">{{ results[port] }}</div>
        </div>
        <div class="row q-gutter-sm"><q-btn flat no-caps label="LAN 어댑터 다시 검색" :disable="busy" @click="refreshAdapters" /><q-btn outline no-caps label="설정 저장" :disable="busy" @click="save" /></div>
        <div v-if="error" class="text-negative" role="alert">{{ error }}</div>
      </q-card-section>
    </q-card>
  </q-dialog>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { masterSerial, type MasterSnapshot, type UartPort } from '../services/master-serial'
import { defaultWolConfig, normalizeWakeMac, wakeResultText, type WolAdapter, type WolConfig } from '../shared/wake'
const STORAGE = 'kbm.pc-wake.v1'
function load(): [WolConfig, WolConfig] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(STORAGE) ?? '')
    if (Array.isArray(value) && value.length === 2) return value.map(item => {
      if (!item || typeof item.enabled !== 'boolean' || typeof item.mac !== 'string' || typeof item.localAddress !== 'string') throw new Error()
      return { enabled: item.enabled, mac: item.mac, localAddress: item.localAddress }
    }) as [WolConfig, WolConfig]
  } catch { /* Invalid saved settings never authorize a wake. */ }
  return [defaultWolConfig(), defaultWolConfig()]
}
const open = ref(false), busy = ref(false), error = ref(''), settings = ref(load())
const results = ref(['', '']), adapters = ref<WolAdapter[]>([]), snapshot = ref<MasterSnapshot>()
const unsubscribe = masterSerial.subscribe(value => { snapshot.value = value })
onBeforeUnmount(unsubscribe)
const adapterOptions = computed(() => adapters.value.map(value => ({ label: `${value.name} · ${value.address} → ${value.broadcast}`, value: value.address })))
function usbAvailable(port: UartPort): boolean {
  return Boolean(snapshot.value?.connected && snapshot.value.protocolVersion === 3 && snapshot.value.online[port] && !masterSerial.isLinkTesting(port) && !masterSerial.isRebooting(port))
}
async function refreshAdapters(): Promise<void> {
  try { adapters.value = await window.kbmNative.wake.adapters(); error.value = '' }
  catch (e) { error.value = String(e) }
}
function save(): void {
  try {
    for (const value of settings.value) if (value.enabled) {
      value.mac = normalizeWakeMac(value.mac)
      if (!adapters.value.some(adapter => adapter.address === value.localAddress)) throw new Error('현재 PC의 실제 LAN 어댑터를 선택하세요.')
    }
    localStorage.setItem(STORAGE, JSON.stringify(settings.value)); error.value = ''
  } catch (e) { error.value = String(e) }
}
async function perform(port: UartPort, action: () => Promise<{ code: number }>): Promise<void> {
  busy.value = true; error.value = ''
  try { const result = await action(); results.value[port] = `응답 코드 ${result.code}: ${wakeResultText(result.code)}` }
  catch (e) {
    error.value = String(e)
    window.kbmNative.logs.record('error', 'ui.wake.failed', { port, error: error.value })
  }
  finally { busy.value = false }
}
async function usbWake(port: UartPort): Promise<void> { await perform(port, () => masterSerial.wakePc(port)) }
async function wolWake(port: UartPort): Promise<void> { await perform(port, () => window.kbmNative.wake.wol(port, { ...settings.value[port] })) }
watch(open, value => { if (value) void refreshAdapters() })
</script>
