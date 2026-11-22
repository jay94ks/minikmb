<template>
  <q-btn flat no-caps icon="monitor_heart" label="HID 상태" @click="open = true">
    <q-badge v-if="alarmCount" color="negative" floating>{{ alarmCount }}</q-badge>
  </q-btn>
  <q-dialog v-model="open">
    <q-card class="hid-diagnostics-card">
      <q-card-section class="row items-center"><div class="text-h6">slave USB · HID 상태와 알람</div><q-space /><q-btn v-close-popup flat round icon="close" /></q-card-section>
      <q-card-section class="text-caption">UART ONLINE은 USB 인식 성공을 의미하지 않습니다. USB 설정과 HID 전송 완료도 대상 OS가 키를 실행했다는 뜻은 아닙니다. 이 화면은 상태만 읽고 키·깨우기·재시작 신호를 보내지 않습니다.</q-card-section>
      <q-card-section class="hid-port-grid">
        <div v-for="(port, index) in state" :key="index" class="hid-port">
          <div class="row items-center q-gutter-sm"><strong>UART {{ index }}</strong><q-badge :color="port.online ? 'teal-7' : 'grey-8'">{{ port.online ? 'ONLINE' : 'OFFLINE' }}</q-badge><q-space /><span class="text-caption">부팅 보고 {{ port.bootCount }}회</span></div>
          <div class="text-caption q-my-sm">{{ port.firmware === undefined ? '부팅 보고 없음' : `펌웨어 0x${port.firmware.toString(16)}` }} · {{ port.seenAt ? `마지막 HID 보고 ${Math.max(0, Math.round((now-port.seenAt)/1000))}초 전` : 'HID 보고 없음 (펌웨어 확인 필요)' }}</div>
          <template v-if="port.snapshot">
            <div class="row q-gutter-xs q-my-sm"><q-badge :color="port.snapshot.mounted ? 'teal-8' : 'grey-8'">USB 설정 {{ yes(port.snapshot.mounted) }}</q-badge><q-badge :color="port.snapshot.ready ? 'teal-8' : 'grey-8'">사용 준비 {{ yes(port.snapshot.ready) }}</q-badge><q-badge :color="port.snapshot.suspended ? 'amber-9' : 'grey-8'">절전 {{ yes(port.snapshot.suspended) }}</q-badge><q-badge :color="port.snapshot.hidReady ? 'teal-8' : 'grey-8'">HID 제출 가능 {{ yes(port.snapshot.hidReady) }}</q-badge></div>
            <dl>
              <dt>HID 큐</dt><dd>{{ port.snapshot.queue }} / 32 · 거절 누계 {{ port.snapshot.queueRejected }}</dd>
              <dt>USB 제출 / 완료</dt><dd>{{ port.snapshot.reportsSubmitted }} / {{ port.snapshot.reportsCompleted }}</dd>
              <dt>대기 보고서</dt><dd>{{ port.snapshot.waiting ? `ID ${port.snapshot.reportId}, ${port.snapshot.waitMs}ms` : '없음' }} · {{ port.snapshot.submitted ? '제출 후 완료 대기' : '미제출' }}</dd>
              <dt>USB reset / setup</dt><dd>{{ port.snapshot.busResets }} / {{ port.snapshot.setups }}</dd>
              <dt>USB 세대 / RX 오류</dt><dd>{{ port.snapshot.usbEpoch }} / {{ port.snapshot.rxErrors }}</dd>
              <dt>가동 시간 / reset 원인</dt><dd>{{ Math.round(port.snapshot.uptimeMs/1000) }}초 · {{ port.snapshot.watchdogReset ? 'watchdog reset' : '전원/기타 (구분 불가)' }}</dd>
            </dl>
          </template>
          <q-banner v-for="alarm in port.alarms" :key="alarm.code" dense class="q-mt-sm" :class="alarm.level === 'error' ? 'bg-red-10' : alarm.level === 'warn' ? 'bg-orange-10' : 'bg-blue-grey-9'">{{ alarm.message }}</q-banner>
          <div v-if="!port.alarms.length" class="text-caption text-grey-5">활성 알람 없음 · 호스트의 실제 키 실행 여부는 미확인</div>
        </div>
      </q-card-section>
      <q-card-section v-if="error" class="text-negative">{{ error }}</q-card-section>
    </q-card>
  </q-dialog>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useQuasar } from 'quasar'
import type { HidDiagnosticsState } from '../shared/hid-diagnostics'
const $q = useQuasar(), open = ref(false), error = ref(''), now = ref(Date.now())
const state = ref<HidDiagnosticsState>([{ online:false,seenAt:0,bootCount:0,alarms:[] },{ online:false,seenAt:0,bootCount:0,alarms:[] }])
const alarmCount = computed(() => state.value.reduce((sum,port) => sum+port.alarms.filter(alarm=>alarm.level!=='info').length,0))
const yes = (value: boolean): string => value ? 'ON' : 'OFF'
let timer: ReturnType<typeof setInterval> | undefined, unsubscribe: (() => void) | undefined, reading = false
async function refresh(): Promise<void> {
  if (reading) return
  reading = true; now.value = Date.now()
  try { state.value = await window.kbmNative.hidDiagnostics.state(); error.value = '' }
  catch (e) { error.value = e instanceof Error ? e.message : String(e) }
  finally { reading = false }
}
onMounted(() => {
  unsubscribe = window.kbmNative.hidDiagnostics.onAlarm(alarm => {
    if (!alarm.recovered && alarm.level !== 'info') $q.notify({type:alarm.level==='error'?'negative':'warning',message:`UART${alarm.port}: ${alarm.message}`,timeout:6000})
    void refresh()
  })
  void refresh(); timer = setInterval(() => void refresh(),1000)
})
onBeforeUnmount(() => { unsubscribe?.(); if (timer) clearInterval(timer) })
</script>
<style scoped lang="scss">
.hid-diagnostics-card { width: 1100px; max-width: 96vw; background: #101b29; }
.hid-port-grid { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 16px; }
.hid-port { padding: 14px; border: 1px solid #33475c; border-radius: 8px; }
dl { display: grid; grid-template-columns: 130px 1fr; gap: 6px 12px; font-size: 12px; }
dt { color: #8fa6bd; } dd { margin: 0; }
@media (max-width: 750px) { .hid-port-grid { grid-template-columns: 1fr; } }
</style>
