<template>
  <q-btn flat no-caps icon="folder_shared" label="가상 디스크" aria-label="가상 디스크" @click="open = true">
    <q-badge v-if="states.some(state => state.enabled)" floating color="teal-7">ON</q-badge>
  </q-btn>
  <q-dialog v-model="open">
    <q-card class="disk-card">
      <q-card-section class="row items-center"><div class="text-h6">폴더 → Slave 읽기 전용 USB 디스크</div><q-space /><q-btn v-close-popup flat round icon="close" aria-label="가상 디스크 닫기" /></q-card-section>
      <q-card-section class="q-gutter-md">
        <div>Slave PC에는 FAT32 이동식 디스크로 보입니다. 폴더 목록은 공유를 켤 때 고정되며, 변경 사항은 수동 새로고침으로 반영합니다. 원본 파일은 수정하지 않습니다.</div>
        <div v-for="port in ports" :key="port" class="disk-port q-gutter-sm">
          <div class="row items-center q-gutter-sm"><strong>UART {{ port }}</strong>
            <q-badge :color="states[port].available ? 'teal-8' : 'grey-8'">{{ states[port].available ? 'ONLINE' : 'OFFLINE' }}</q-badge>
            <q-badge :color="states[port].phase === 'active' ? 'teal-8' : 'grey-8'">{{ states[port].phase === 'hidden' && !states[port].device ? 'OFF · 상태 확인 중' : phaseLabel(states[port].phase) }}</q-badge>
            <q-space />
            <q-btn outline no-caps label="폴더 선택" :disable="states[port].busy || states[port].enabled" @click="choose(port)" />
            <q-btn color="cyan-8" no-caps :label="states[port].enabled ? '공유 끄기' : '공유 켜기'" :loading="states[port].busy" :disable="!connected || !states[port].available || (!states[port].enabled && !folders[port])" @click="toggle(port)" />
            <q-btn outline no-caps icon="refresh" label="새로고침 / 다시 연결" :disable="!states[port].snapshot || states[port].busy || !states[port].available" @click="session.start(port)" />
          </div>
          <div class="folder-path">{{ folders[port] || '공유할 폴더를 선택하세요.' }}</div>
          <div v-if="states[port].snapshot" class="text-caption">파일 {{ states[port].snapshot?.files }}개 · {{ ((states[port].snapshot?.bytes ?? 0) / 1048576).toFixed(2) }} MiB · 읽은 섹터 {{ metrics[port]?.sectors ?? 0 }}</div>
          <div v-if="metrics[port]?.sectors" class="text-caption">{{ speed(port) }} KiB/s · 섹터 평균: 원본 {{ average(port, 'sourceMs') }} ms / 전송 ACK {{ average(port, 'transferMs') }} ms · 재요청 대기 {{ metrics[port]?.duplicates ?? 0 }} · 오류 {{ metrics[port]?.errors ?? 0 }}</div>
          <div class="text-caption">Slave 상태: {{ states[port].device ? (states[port].device?.ready ? '공유 매체 준비됨' : '스토리지 숨김') : '미확인' }} · 로컬 서비스 {{ states[port].enabled ? 'ON' : 'OFF' }}</div>
          <div v-if="states[port].error || errors[port]" class="text-negative" role="alert">{{ states[port].error || errors[port] }}</div>
        </div>
        <div class="text-caption text-grey-5">매 실행 OFF이며 스토리지 장치 자체를 숨깁니다. 공유 ON일 때만 표시되고, 앱/연결 종료 후 5초 내 제거됩니다. 켜고 끌 때 USB 재연결로 HID·시리얼·NCM도 잠시 끊깁니다. 공유 중 수정·교체된 파일은 읽기 오류 처리합니다. 심볼릭 링크는 제외합니다. 최대 10,000개 항목, 파일당 4 GiB 미만, 디스크 32 GiB. UART 기반이므로 큰 파일 복사는 느리며 HID가 우선합니다.</div>
        <div class="text-caption text-grey-5">준비 상태는 DISK_STATUS로 확인하며, 원격 OS의 드라이브 문자/마운트 완료를 뜻하지 않습니다. 꺼내기·연결 끊김 후 자동으로 다시 공유하지 않습니다. 네이티브 NCM 모드에서도 폴더 공유는 이 앱을 계속 실행해야 합니다.</div>
      </q-card-section>
    </q-card>
  </q-dialog>
</template>
<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { masterSerial, type UartPort } from '../services/master-serial'
import { DiskSession, type DiskPhase } from '../services/disk-session'
import type { DiskTransferMetrics } from '../shared/disk'
defineProps<{ connected: boolean }>()
const ports = [0, 1] as const
const open = ref(false), folders = ref<[string, string]>(['', ''])
const errors = ref<[string, string]>(['', ''])
const metrics = ref<DiskTransferMetrics[]>([])
function speed(port: UartPort): string { const m = metrics.value[port]; return m?.totalMs ? (m.sectors * 500 / m.totalMs).toFixed(1) : '0.0' }
function average(port: UartPort, key: 'sourceMs' | 'transferMs'): string { const m = metrics.value[port]; return m?.sectors ? (m[key] / m.sectors).toFixed(2) : '0.00' }
const session = new DiskSession({
  prepare: port => window.kbmNative.disk.prepare(port),
  disable: port => window.kbmNative.disk.disable(port),
  read: (port, request) => window.kbmNative.disk.read(port, request),
  configure: (port, generation, sectors, enabled) => masterSerial.diskConfigure(port, generation, sectors, enabled),
  status: port => masterSerial.diskStatus(port),
  data: (port, request, bytes, current) => masterSerial.diskData(port, request, bytes, current),
  fail: (port, request) => masterSerial.diskFail(port, request)
}, () => { states.value = session.ports.map(state => ({ ...state })) as typeof session.ports })
const states = ref(session.ports.map(state => ({ ...state })) as typeof session.ports)
function phaseLabel(phase: DiskPhase): string {
  return { hidden: 'OFF · 장치 숨김', starting: '공유 준비 중', active: 'ON · 읽기 전용',
    stopping: '숨김 확인 중', unavailable: '연결 안 됨', ejected: '꺼내기 / lease 만료', error: '오류' }[phase]
}
let timer: ReturnType<typeof setInterval> | undefined
let unsubscribeMaster: (() => void) | undefined
let metricsBusy = false
const message = (error: unknown): string => error instanceof Error ? error.message : String(error)
async function choose(port: UartPort): Promise<void> {
  try { const folder = await window.kbmNative.disk.choose(port); if (folder) folders.value[port] = folder }
  catch (error) { errors.value[port] = message(error) }
}
async function toggle(port: UartPort): Promise<void> {
  errors.value[port] = ''
  if (session.ports[port].enabled) await session.stop(port)
  else await session.start(port)
}
onMounted(() => {
  // Main owns F8/sector/fragment ACKs; renderer only handles user controls/status.
  unsubscribeMaster = masterSerial.subscribe(state => {
    for (const port of ports) session.observe(port, state.connected && state.protocolVersion === 3 && state.online[port])
  })
  timer = setInterval(() => {
    for (const port of ports) void session.tick(port)
    if (!metricsBusy) {
      metricsBusy = true
      void window.kbmNative.disk.metrics().then(value => { metrics.value = value }).catch(() => {}).finally(() => { metricsBusy = false })
    }
  }, 1000)
})
onBeforeUnmount(() => {
  if (timer) clearInterval(timer)
  unsubscribeMaster?.()
  for (const port of ports) void session.stop(port).catch(() => {})
})
</script>
<style scoped lang="scss">
.disk-card { width: 940px; max-width: 96vw; background: #101b29; }
.disk-port { padding: 16px; border: 1px solid #345; border-radius: 8px; }
.folder-path { overflow-wrap: anywhere; color: #9fc7df; }
</style>
