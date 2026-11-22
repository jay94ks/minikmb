<template>
  <q-btn flat no-caps icon="developer_board" label="보드 관리" aria-label="보드 관리" @click="open = true" />
  <q-dialog v-model="open" :persistent="running">
    <q-card style="width:720px;max-width:95vw;background:#101b29">
      <q-card-section class="row items-center"><div class="text-h6">Slave 보드 관리 · UART 진단</div><q-space />
        <q-btn v-close-popup flat round icon="close" :disable="running" aria-label="UART 시험 닫기" />
      </q-card-section>
      <q-card-section class="q-gutter-md">
        <div class="row q-gutter-sm"><q-badge :color="connected ? 'teal-8' : 'grey-8'">Master {{ connected ? '연결됨' : '연결 안 됨' }}</q-badge><q-badge outline>프로토콜 v{{ snapshot?.protocolVersion ?? 0 }} · UART 921600-8-N-1</q-badge></div>
        <q-markup-table dark flat>
          <thead><tr><th>대상</th><th>상태</th><th>USB</th><th>DTR / RTS</th><th>TX / RX frame</th><th>디코드 오류</th></tr></thead>
          <tbody><tr v-for="p in [0, 1] as const" :key="p"><td>UART {{ p }}</td><td>{{ snapshot?.online[p] ? 'ONLINE' : 'OFFLINE' }}</td><td>{{ (snapshot?.serialFlags[p] ?? 0) & 1 ? '연결됨' : '대기' }}</td><td>{{ (snapshot?.serialFlags[p] ?? 0) & 2 ? 'ON' : 'OFF' }} / {{ (snapshot?.serialFlags[p] ?? 0) & 4 ? 'ON' : 'OFF' }}</td><td>{{ snapshot?.diagnostics[p].txFrames ?? 0 }} / {{ snapshot?.diagnostics[p].rxFrames ?? 0 }}</td><td>{{ snapshot?.diagnostics[p].decodeErrors ?? 0 }}</td></tr></tbody>
        </q-markup-table>
        <div class="text-caption">DTR/RTS는 slave PC 응용 프로그램이 COM 포트를 열었는지 보여주며 HID·스토리지·NCM의 사용 조건은 아닙니다.</div>
        <q-separator dark />
        <div>ADuM1201ARZ 정격 상한 1 Mbps. 115200 → 230400 → 460800 → 921600 → 1000000 bps를 시험합니다.</div>
        <div class="text-caption">운영 기본값과 임시 시험 후 복귀 속도는 921600 bps입니다.</div>
        <div class="text-caption text-grey-5">선택한 UART의 네트워크·포워딩을 끄며, 시험 동안 HID 입력을 차단합니다. slave COM은 닫아주세요. 각 속도는 10초 후 자동 복귀하며 영구 설정은 변경하지 않습니다.</div>
        <q-select v-model="port" :options="options" emit-value map-options label="시험 대상" outlined dark :disable="running" />
        <q-btn unelevated color="cyan-8" no-caps label="패턴 왕복 시험 시작" :disable="!connected || !snapshot?.online[port] || nativeOwned" :loading="running" @click="confirmTest = true" />
        <q-btn outline color="orange-5" no-caps label="선택한 Slave RP2040 재부팅" :disable="!connected || !snapshot?.online[port] || running" @click="rebootTarget = port; confirmReboot = true" />
        <div v-if="nativeOwned" class="text-orange-4">네이티브 NCM 모드/전환 중에는 UART 속도 시험을 사용할 수 없습니다.</div>
        <q-linear-progress v-if="running" indeterminate color="cyan-5" />
        <div aria-live="polite">{{ message }}</div>
        <q-markup-table v-if="results.length" dark flat>
          <thead><tr><th>Baud</th><th>패턴</th><th>오류/타임아웃</th><th>소요(ms)</th></tr></thead>
          <tbody><tr v-for="row in results" :key="row.baud"><td>{{ row.baud }}</td><td>{{ row.passed }}/64</td><td>{{ row.errors }}</td><td>{{ row.elapsedMs }}</td></tr></tbody>
        </q-markup-table>
        <div class="text-caption text-grey-5">양쪽 포트를 각각 시험하고 장시간 부하·케이블 조건까지 확인한 뒤 운영 속도를 결정하세요.</div>
      </q-card-section>
    </q-card>
  </q-dialog>
  <q-dialog v-model="confirmReboot">
    <q-card style="background:#101b29">
      <q-card-section class="text-h6">UART {{ rebootTarget }} Slave RP2040 재부팅</q-card-section>
      <q-card-section>연결된 PC가 아니라 이 UART의 slave 보드만 재부팅합니다. HID/시리얼이 잠시 끊기며, 사용 중인 네트워크의 IP 임대도 다시 시작합니다. 가상 디스크 공유는 중단되며 수동으로 다시 켜야 합니다. 다른 UART는 재부팅하지 않습니다.</q-card-section>
      <q-card-actions align="right"><q-btn v-close-popup flat label="취소" /><q-btn v-close-popup color="orange-9" label="보드 재부팅" @click="reboot" /></q-card-actions>
    </q-card>
  </q-dialog>
  <q-dialog v-model="confirmTest"><q-card style="background:#101b29"><q-card-section class="text-h6">UART {{ port }} 속도 시험</q-card-section><q-card-section>최대 약 55초 동안 해당 slave의 입력과 네트워크를 중단합니다. 스토리지 공유를 먼저 끄고 slave COM 포트를 닫아주세요. 실행하시겠습니까?</q-card-section><q-card-actions align="right"><q-btn v-close-popup flat label="취소" /><q-btn v-close-popup color="cyan-8" label="시험 실행" @click="run" /></q-card-actions></q-card></q-dialog>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { masterSerial, type MasterSnapshot, type NativeModeState, type UartPort } from '../services/master-serial'
defineProps<{ connected: boolean }>()
const emit = defineEmits<{ testing: [port: UartPort | undefined] }>()
const open = ref(false), running = ref(false), port = ref<UartPort>(0), message = ref('')
const confirmReboot = ref(false)
const confirmTest = ref(false), rebootTarget = ref<UartPort>(0)
const snapshot = ref<MasterSnapshot>(), native = ref<NativeModeState>()
const nativeOwned = computed(() => Boolean(native.value?.enabled || native.value?.visible || native.value?.transition))
let unsubscribe: (() => void) | undefined
let timer: ReturnType<typeof setInterval> | undefined
onMounted(() => {
  unsubscribe = masterSerial.subscribe(state => { snapshot.value = state })
  timer = setInterval(() => {
    if (open.value && snapshot.value?.connected && !running.value) void masterSerial.queryNativeMode().then(state => { native.value = state }).catch(() => {})
  }, 1000)
})
onBeforeUnmount(() => { unsubscribe?.(); if (timer) clearInterval(timer) })
const results = ref<Array<{ baud: number; passed: number; errors: number; elapsedMs: number }>>([])
const options = [{ label: 'UART 0', value: 0 }, { label: 'UART 1', value: 1 }]
async function run(): Promise<void> {
  running.value = true; results.value = []; emit('testing', port.value)
  try { results.value = await masterSerial.testBaudRates(port.value, text => { message.value = text }) }
  catch (error) { message.value = error instanceof Error ? error.message : String(error) }
  finally { running.value = false; emit('testing', undefined) }
}
async function reboot(): Promise<void> {
  running.value = true; emit('testing', rebootTarget.value)
  try { await masterSerial.rebootSlave(rebootTarget.value); message.value = `UART ${rebootTarget.value} RP2040 재부팅 후 UART 응답을 확인했습니다.` }
  catch (error) { message.value = `재부팅 자동 재시도 없음: ${error instanceof Error ? error.message : String(error)}` }
  finally { running.value = false; emit('testing', undefined) }
}
</script>
