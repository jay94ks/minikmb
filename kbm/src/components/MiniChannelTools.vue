<template>
  <section :class="console ? 'console-tools' : 'channel-tools q-mt-lg'">
    <component v-if="!console || tab !== 'keyboard' || keyboardVisible" :is="console && tab !== 'keyboard' ? QDialog : 'div'" v-model="dialogOpen">
    <q-card flat class="tools-card" :class="{ 'modal-card': console && tab !== 'keyboard' }">
    <div class="row items-center q-pa-sm"><strong>{{ console && tab === 'keyboard' ? '가상 키보드' : '선택 채널 도구' }}</strong><q-space /><OperationLogs v-if="!console" /><q-btn flat dense icon="refresh" :loading="polling" @click="refresh" /><q-btn v-if="console" flat dense icon="close" @click="closePanel" /></div>
    <q-tabs v-if="!console" v-model="tab" dense align="left" active-color="cyan-4" outside-arrows mobile-arrows>
      <q-tab name="keyboard" label="가상 키보드 · 특수 입력" /><q-tab name="storage" label="SD · MSC 공유" />
      <q-tab name="serial" label="시리얼 터미널" /><q-tab name="network" label="네트워크 · 포워딩" /><q-tab name="wake" label="WOL" /><q-tab name="diagnostics" label="진단" />
    </q-tabs>
    <q-banner v-if="error" dense class="bg-red-10">{{ error }}</q-banner>
    <q-tab-panels v-model="tab" animated class="bg-transparent">
      <q-tab-panel name="keyboard">
        <div class="row items-center q-gutter-sm q-mb-md"><q-chip :color="hid ? 'teal-9' : 'grey-9'">{{ hid ? 'HID 지원' : '현재 펌웨어 HID 비활성' }}</q-chip>
          <q-btn outline label="모든 입력 해제" :disable="!hid || busy" @click="inputRelease" />
        </div>
        <div class="row q-gutter-xs q-mb-md"><q-btn v-for="p in specialPresets" :key="p.label" dense outline no-caps :label="p.label" :disable="!canType" @click="chord({ modifiers: p.modifiers, key: p.key }, true)" /></div>
        <div class="row q-gutter-sm q-mb-md"><q-btn v-for="m in virtualModifiers" :key="m.mask" :color="modifiers & m.mask ? 'teal-8' : 'grey-9'" :label="m.label" @click="modifiers ^= m.mask" />
          <q-select dark dense outlined emit-value map-options v-model="customKey" label="사용자 지정 키" :options="specialKeyOptions" style="min-width:190px" />
          <q-btn outline label="조합 전송" :disable="!canType" @click="chord({ modifiers, key: customKey }, true)" />
        </div>
        <div v-for="(row, i) in virtualKeyboardRows" :key="i" class="keyboard-row"><q-btn v-for="key in row" :key="key.code" dense no-caps outline :label="key.label" :style="{ flex: key.width ?? 1 }" :disable="!canType" @click="chord({ modifiers, key: key.code })" /></div>
        <div class="text-caption text-grey-5 q-mt-sm">수정키는 다음 한 번의 조합에만 적용합니다.</div>
        <q-expansion-item label="System Control 입력" class="q-mt-md">
          <p class="text-caption q-pa-sm">현재 연결된 장치에서는 지원하지 않는 기능입니다.</p>
          <div class="row q-gutter-xs"><q-btn v-for="control in systemControls" :key="control.usage" dense outline no-caps :label="control.label" disable /></div>
        </q-expansion-item>
      </q-tab-panel>
      <q-tab-panel name="storage">
        <div class="storage-layout"><div class="storage-functions">
        <h3>{{ selectedImage ? selectedImage.name : '이미지를 선택하세요' }}</h3>
        <template v-if="selectedImage?.kind === 'optical'">
        <q-toggle :model-value="state?.storage.opticalApplied ?? true" label="광학 드라이브 에뮬레이션" :disable="busy || !(state?.storage.capabilities && (state.storage.capabilities & 1)) || Boolean(state?.storage.disk?.lease)" @update:model-value="value => storage({ kind: 'optical-set', enabled: Boolean(value) })" />
        <p class="text-caption">광학을 끄면 타깃의 광학 드라이브 장치가 사라집니다. 변경 시 타깃 USB가 잠시 재연결됩니다. 연결된 이동식 디스크는 먼저 안전하게 분리하세요.</p>
        <p class="text-caption">광학 업로드는 기존 활성 이미지를 교체하며 다른 데이터 기능을 중단합니다.</p>
        <div class="row q-gutter-sm q-mb-md"><q-btn outline label="SD 상태 읽기" :disable="busy" @click="storage({ kind: 'refresh' })" />
          <q-btn outline label="선택 광학 연결" :disable="busy || selectedImage?.kind !== 'optical'" @click="storage({ kind: 'optical-mount' })" />
          <q-btn outline label="광학 매체 분리" :disable="busy || selectedImage?.kind !== 'optical'" @click="storage({ kind: 'optical-unmount' })" />
        </div>
        <div v-if="state?.storage.image" class="text-caption q-mb-md">광학: {{ state.storage.image.name }} · {{ bytes(state.storage.image.size) }} · {{ state.storage.image.mounted ? '타깃 연결됨' : '분리됨' }}</div>
        </template>
        <div v-if="state?.storage.busy" class="q-mb-md"><div>{{ state.storage.action }} · {{ bytes(state.storage.done) }} / {{ bytes(state.storage.total) }}</div>
          <div v-if="state.storage.transfer" class="text-caption q-mt-xs">전송 속도 {{ transferRate(state.storage.transfer.bytesPerSecond) }} · 남은 전송 예상 시간 {{ transferTime(state.storage.transfer.remainingSeconds) }}</div>
          <q-linear-progress color="cyan" :indeterminate="!state.storage.total" :value="state.storage.total ? state.storage.done / state.storage.total : 0" />
          <q-btn flat label="작업 취소" @click="cancelStorage" />
        </div>
        <template v-if="selectedImage?.kind === 'disk'">
        <div class="row q-gutter-sm items-center q-mb-md"><q-select dark dense outlined v-model="diskFormat" :options="formats" emit-value map-options label="이미지 파일시스템" style="width:160px" />
          <q-btn outline color="orange" label="포맷하기" :disable="busy || selectedImage?.kind !== 'disk' || selectedImage.mounted || !(state?.storage.capabilities && (state.storage.capabilities & 4))" @click="formatImage" />
          <span class="text-caption">포맷하면 선택한 이미지의 내용이 모두 삭제됩니다.</span>
        </div>
        <div class="row q-gutter-sm items-center q-mb-md"><q-input dark dense outlined :model-value="selectedImage?.name ?? ''" label="선택한 이미지" readonly style="width:180px" />
          <q-select dark dense outlined v-model="diskPath" :options="paths" emit-value map-options label="접근 경로" style="width:200px" /><q-toggle v-model="writable" label="쓰기 허용" />
          <q-btn outline label="선택 이미지 열기" :disable="busy || selectedImage?.kind !== 'disk'" @click="openImage" />
          <q-btn outline label="동기화" :disable="busy || !selectedDisk" @click="storage({ kind: 'disk-sync' })" />
          <q-btn outline label="경로 닫기" :disable="busy || !selectedDisk" @click="storage({ kind: 'disk-close' }, true)" />
        </div>
        <p class="text-caption">{{ diskDescription }}. 경로를 바꾸기 전에 연결된 OS에서 복사와 안전 제거를 마치세요. 명령 완료는 OS 마운트 완료를 뜻하지 않습니다.</p>
        <q-expansion-item label="관리 직접 섹터 I/O (OS 마운트 없이 읽기/쓰기)">
          <div class="row q-gutter-sm q-mt-sm"><q-input dark dense outlined type="number" v-model.number="lba" label="LBA" />
            <q-btn outline label="512B 읽기" :disable="busy || selectedDisk?.state !== 3" @click="readSector" /><q-btn outline label="512B 쓰기 · 동기화" :disable="busy || selectedDisk?.state !== 3 || !selectedDisk?.writable" @click="writeSector" /></div>
          <q-input dark outlined type="textarea" v-model="sectorHex" label="정확히 512바이트 HEX" class="q-mt-sm mono" />
        </q-expansion-item>
        </template>
        <p v-if="!selectedImage" class="text-grey-5">오른쪽 목록에서 이미지를 선택하거나 + 메뉴로 추가하세요.</p>
        <q-expansion-item label="폴더 스냅샷 공유" class="q-mt-md"><p>폴더를 읽기 전용 FAT32 이미지로 SD에 기록합니다. 파일 변경은 다시 공유해야 반영됩니다.</p><q-btn outline label="폴더 선택 · 스냅샷 공유" :disable="busy" @click="storage({ kind: 'folder-share' }, true)" /></q-expansion-item>
        <q-expansion-item label="사용자 식별값 · SD 포맷" class="q-mt-md">
          <p class="text-caption">{{ state?.storage.card?.present ? '카드 있음' : '카드 상태를 읽으세요' }}</p>
          <div class="row q-gutter-sm">
            <q-btn outline label="사용자 값 읽기" :disable="busy" @click="readUser" /><q-btn outline label="사용자 값 저장" :disable="busy" @click="storage({ kind: 'card-user-write', text: userText }, true)" /></div>
          <q-input dark outlined type="textarea" v-model="userText" label="사용자 식별값 (UTF-8 최대 4 KiB)" class="q-mt-sm" />
          <div class="row q-gutter-sm q-mt-md"><q-btn outline color="orange" label="카드 전체 삭제 · FAT32" :disable="busy" @click="formatCard(1)" /><q-btn outline color="orange" label="카드 전체 삭제 · exFAT" :disable="busy" @click="formatCard(2)" /></div>
        </q-expansion-item>
        </div><aside class="storage-images">
          <div class="row items-center"><h3>SD 카드 이미지</h3><q-space /><q-btn dense flat icon="refresh" :loading="busy" label="목록 갱신" @click="storage({ kind: 'refresh' })" />
            <q-btn dense flat icon="add" aria-label="이미지 추가" :disable="busy"><q-menu dark><q-list>
              <q-item clickable v-close-popup @click="storage({ kind: 'optical-upload' }, true)"><q-item-section avatar><q-icon name="upload_file" /></q-item-section><q-item-section>부팅 ISO/UDF 업로드</q-item-section></q-item>
              <q-item v-for="format in formats" :key="format.value" clickable v-close-popup @click="createImage(format.value)"><q-item-section avatar><q-icon name="add_circle_outline" /></q-item-section><q-item-section>{{ format.label }} 이미지 만들기</q-item-section></q-item>
            </q-list></q-menu></q-btn>
          </div>
          <p class="text-caption text-grey-5">{{ state?.storage.images.length ?? 0 }}개 · 이미지를 선택한 뒤 왼쪽 기능을 사용하세요.</p>
          <q-list bordered separator dark>
            <q-item v-for="image in state?.storage.images" :key="`${image.kind}:${image.id}`" clickable :active="selectedImageKey === `${image.kind}:${image.id}`" active-class="selected-image" @click="selectImage(image)">
              <q-item-section avatar><q-icon :name="image.kind === 'optical' ? 'album' : 'storage'" color="cyan-4" /></q-item-section>
              <q-item-section><q-item-label>{{ image.name }}</q-item-label><q-item-label caption>{{ formatName(image.format) }} · {{ bytes(image.size) }}</q-item-label>
                <q-item-label caption>{{ image.mounted ? (image.path === 1 ? '타깃 연결됨' : image.path === 2 ? '관리 PC 연결됨' : '관리 직접 접근 중') : '사용 가능 · 분리됨' }}{{ image.writable ? ' · RW' : ' · RO' }}</q-item-label></q-item-section>
            </q-item>
          </q-list>
          <p v-if="!state?.storage.images.length" class="text-grey-5">등록된 이미지가 없습니다. 목록을 갱신하거나 새 이미지를 만드세요.</p>
          <p v-if="state && !(state.storage.capabilities & 2)" class="text-caption text-orange">전체 SD 목록을 읽으려면 목록 기능을 지원하는 MCU 펌웨어가 필요합니다.</p>
        </aside></div>
      </q-tab-panel>
      <q-tab-panel name="serial">
        <div class="row q-gutter-sm items-center"><q-btn outline label="수신 시작" :disable="busy" @click="run(() => api.terminal(uid))" />
          <q-toggle v-model="serialHex" label="HEX" /><q-toggle v-model="newline" label="송신 끝 CRLF" /><q-btn flat label="표시 지우기" @click="serialCleared = state?.serial.bytesReceived ?? 0" /></div>
        <p class="text-caption">{{ state?.serial.open ? '타깃 CDC 열림' : '타깃 CDC 닫힘' }} · {{ state?.serial.baud }} baud · {{ state?.serial.dataBits }} data bits · parity {{ state?.serial.parity }} / stop {{ state?.serial.stopBits }} · TX/RX 큐 {{ state?.serial.txQueued }}/{{ state?.serial.rxQueued }}</p>
        <pre class="terminal">{{ serialDisplay }}</pre>
        <div class="row q-gutter-sm"><q-input dark dense outlined v-model="serialText" :label="serialHex ? '송신 HEX (공백 허용)' : '송신 텍스트'" class="col" @keyup.enter="serialSend" /><q-btn color="teal-9" label="전송" :disable="busy" @click="serialSend" /></div>
      </q-tab-panel>
      <q-tab-panel name="network">
        <div class="row q-gutter-sm items-center"><q-btn :color="state?.networkMode === 'emulation' ? 'orange-9' : 'teal-9'" :label="state?.networkMode === 'emulation' ? '에뮬레이션 모드 끄기' : '에뮬레이션 모드로 켜기'" :disable="busy" @click="run(() => api.network(uid, state?.networkMode === 'emulation' ? 'off' : 'emulation'))" />
          <q-btn :color="state?.networkMode === 'host' ? 'orange-9' : 'teal-9'" :label="state?.networkMode === 'host' ? '호스트 네트워크 공유 끄기' : '호스트 네트워크 공유 켜기'" :disable="busy" @click="run(() => api.network(uid, state?.networkMode === 'host' ? 'off' : 'host'))" />
          <span>링크 {{ state?.networkUp ? 'UP' : 'DOWN' }} · RX {{ state?.network.rxFrames[0] ?? 0 }} / TX {{ state?.network.txFrames[0] ?? 0 }} · 오류 {{ state?.networkDropped ?? 0 }}</span></div>
        <div class="row q-gutter-sm q-mt-md"><q-input dark dense outlined v-model="config.serverIp" label="게이트웨이 IPv4 (.1/24)" /><q-input dark dense outlined type="number" v-model.number="config.poolStart" label="DHCP 시작" /><q-input dark dense outlined type="number" v-model.number="config.poolEnd" label="DHCP 끝" /><q-input dark dense outlined type="number" v-model.number="config.leaseSeconds" label="임대 초" />
          <q-btn outline label="설정 저장" :disable="busy || state?.network.enabled[0]" @click="saveNetwork" /></div>
        <p v-if="state?.networkMode === 'host'" class="text-caption">관리 PC의 인터넷·LAN을 IPv4 TCP/UDP와 DNS로 공유합니다.</p>
        <div v-for="lease in state?.network.leases" :key="lease.mac" class="text-caption">DHCP {{ lease.ip }} · {{ lease.mac }}</div>
        <div v-for="(rule, i) in rules" :key="rule.id" class="row q-gutter-sm q-mt-sm"><q-select dark dense outlined v-model="rule.protocol" :options="['tcp', 'udp']" style="width:100px" /><q-input dark dense outlined type="number" v-model.number="rule.listenPort" label="127.0.0.1 포트" /><q-input dark dense outlined v-model="rule.targetIp" label="타깃 IP" /><q-input dark dense outlined type="number" v-model.number="rule.targetPort" label="타깃 포트" /><q-btn flat icon="delete" @click="rules.splice(i, 1)" /></div>
        <div class="row q-gutter-sm q-mt-md"><q-btn outline label="규칙 추가" @click="addRule" :disable="rules.length >= 32" /><q-btn outline label="포워딩 시작" :disable="busy || !state?.network.enabled[0]" @click="startForward" /><q-btn outline label="포워딩 중지" :disable="busy" @click="run(() => api.stopForward(uid))" /></div>
        <div class="text-caption q-mt-sm">{{ state?.network.forwarding ? '포워딩 동작 중' : '포워딩 중지됨' }} · {{ state?.network.connections }} 연결 · {{ state?.network.error }}</div>
        <q-separator class="q-my-md" dark />
        <h3 class="text-subtitle1">타깃 포트 스캐너</h3>
        <div class="row q-gutter-sm items-center"><q-input dark dense outlined v-model="scanTarget" :placeholder="scanDefault" label="타깃 IP" style="width:180px" />
          <q-input dark dense outlined type="number" v-model.number="scanStart" label="시작 포트" style="width:110px" /><q-input dark dense outlined type="number" v-model.number="scanEnd" label="끝 포트" style="width:110px" />
          <q-input dark dense outlined type="number" v-model.number="scanTimeout" label="대기 (ms)" style="width:120px" />
          <q-btn outline label="스캔 시작" :disable="busy || !state?.network.enabled[0] || state?.scan?.running" @click="run(() => api.scan(uid, { target: scanTarget || scanDefault, start: scanStart, end: scanEnd, timeoutMs: scanTimeout }))" />
          <q-btn outline label="스캔 중지" :disable="!state?.scan?.running" @click="run(() => api.scan(uid, null))" />
        </div>
        <div v-if="state?.scan?.total" class="q-mt-sm">{{ state.scan.completed }} / {{ state.scan.total }} · 열림 {{ state.scan.open.length }} · 닫힘 {{ state.scan.closed }} · 응답 없음 {{ state.scan.filtered }}<q-linear-progress :value="state.scan.completed / state.scan.total" color="cyan" class="q-my-sm" /><q-chip v-for="port in state.scan.open" :key="port" color="teal-9" dense>TCP {{ port }}</q-chip></div>
        <div v-if="state?.scan?.error" class="text-orange q-mt-sm">{{ state.scan.error }}</div>
      </q-tab-panel>
      <q-tab-panel name="wake">
        <p>타깃 PC 유선 LAN으로 WOL 패킷을 전송합니다. NCM과 독립적이며, 전송 성공이 PC 기상을 보장하지 않습니다.</p>
        <div class="row q-gutter-sm"><q-select dark dense outlined v-model="wol.localAddress" :options="adapters.map(a => ({ label: `${a.name} · ${a.address}`, value: a.address }))" emit-value map-options label="발신 LAN" style="min-width:270px" /><q-input dark dense outlined v-model="wol.mac" label="타깃 유선 LAN MAC" /><q-toggle v-model="wol.enabled" label="WOL 사용" /><q-btn outline label="깨우기" :disable="busy" @click="wake" /></div>
      </q-tab-panel>
      <q-tab-panel name="diagnostics"><pre>{{ diagnosticText }}</pre><p class="text-caption">관리 CDC·HID 실행 완료·캡처 프레임과 실제 OS 동작을 구분합니다. 현재 채널의 UID와 카운터만 표시하며 다른 MCU로 명령을 넘기지 않습니다.</p></q-tab-panel>
    </q-tab-panels>
    <div v-if="message" class="q-pa-sm text-cyan-3">{{ message }}</div>
    </q-card></component>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from 'vue'
import { QDialog, useQuasar } from 'quasar'
import OperationLogs from './OperationLogs.vue'
import { virtualKeyboardRows, virtualModifiers } from '../shared/virtual-keyboard'
import { specialPresets, specialKeyOptions, type KeyChordRequest } from '../shared/special-input'
import { systemControls } from '../shared/system-controls'
import type { MiniToolsState, MiniStorageAction, MiniDiskImage, ForwardRule } from '../shared/mini-tools'
import { defaultNetworkConfig } from '../shared/network'
import { uidHex } from '../shared/minikmb'
import { defaultWolConfig, wakeResultText, type WolAdapter } from '../shared/wake'
const props = defineProps<{ uid: string; beforeInput: () => Promise<void>; console?: boolean }>()
const dialogOpen = ref(false), keyboardVisible = ref(!props.console)
const uid = props.uid, api = window.kbmNative.mini.tools, $q = useQuasar()
const state = shallowRef<MiniToolsState>(), polling = ref(false), busy = ref(false), error = ref(''), message = ref(''), tab = ref('keyboard')
const modifiers = ref(0), customKey = ref('Delete'), serialText = ref(''), serialHex = ref(false), newline = ref(false), serialCleared = ref(0)
const diskFormat = ref<1 | 2 | 3>(1), diskPath = ref<0 | 1 | 2>(1), writable = ref(false), lba = ref(0), sectorHex = ref(''), userText = ref('')
const selectedImageKey = ref('')
const selectedImage = computed(() => state.value?.storage.images.find(image => `${image.kind}:${image.id}` === selectedImageKey.value))
const selectedDisk = computed(() => { const disk = state.value?.storage.disk; return selectedImage.value?.kind === 'disk' && disk?.id === selectedImage.value.id && disk.lease ? disk : null })
const formatName = (format: number) => ['광학 ISO/UDF', 'FAT32', 'exFAT', 'EXT4'][format] ?? '알 수 없음'
function selectImage(image: MiniDiskImage): void { selectedImageKey.value = `${image.kind}:${image.id}`; if (image.kind === 'disk') diskFormat.value = image.format as 1 | 2 | 3 }
const formats: { label: string; value: 1 | 2 | 3 }[] = [{ label: 'FAT32', value: 1 }, { label: 'EXT4', value: 3 }, { label: 'exFAT', value: 2 }]
const paths = [{ label: '타깃 USB MSC', value: 1 }, { label: '관리 USB MSC', value: 2 }, { label: '관리 직접 섹터 I/O', value: 0 }]
const config = ref({ ...defaultNetworkConfig }), rules = ref<ForwardRule[]>([]), wol = ref(defaultWolConfig()), adapters = ref<WolAdapter[]>([])
const scanTarget = ref(''), scanStart = ref(1), scanEnd = ref(1024), scanTimeout = ref(1000)
const scanDefault = computed(() => state.value?.network.leases?.[0]?.ip ?? config.value.serverIp.replace(/\.1$/, `.${config.value.poolStart}`))
const savedKey = `minikmb.settings.${uid}`
try { const saved = JSON.parse(localStorage.getItem(savedKey) ?? 'null'); if (saved?.config) config.value = saved.config; if (Array.isArray(saved?.rules)) rules.value = saved.rules.slice(0, 32); if (saved?.wol) wol.value = saved.wol } catch { /* Defaults remain OFF. */ }
let disposed = false, timer: ReturnType<typeof setInterval> | undefined, configured = false
function show(name: string): void {
  if (name === 'keyboard') { keyboardVisible.value = true; tab.value = name; dialogOpen.value = false }
  else { keyboardVisible.value = false; tab.value = name; dialogOpen.value = true; if (name === 'storage') void storage({ kind: 'refresh' }) }
}
function closePanel(): void { keyboardVisible.value = false; dialogOpen.value = false }
defineExpose({ show })
const hid = computed(() => Boolean(state.value?.features && (state.value.features & 1)))
const canType = computed(() => hid.value && !busy.value && !state.value?.input.busy)
const serialDisplay = computed(() => {
  const serial = state.value?.serial
  if (!serial) return ''
  const count = Math.max(0, Math.min(serial.received.length, serial.bytesReceived - serialCleared.value)), data = Uint8Array.from(serial.received.slice(-count || serial.received.length))
  return serialHex.value ? Array.from(data, b => b.toString(16).padStart(2, '0')).join(' ') : new TextDecoder().decode(data)
})
const diskDescription = computed(() => { const d = selectedDisk.value; return d ? `디스크 ${d.id} · ${bytes(d.sectors * 512)} · 리스 ${d.lease} · 상태 ${d.state} · ${d.writable ? 'RW' : 'RO'}` : '선택 이미지의 접근 경로가 닫혀 있습니다' })
const diagnosticText = computed(() => JSON.stringify({ uid: uidHex(uid), features: state.value?.features, ...state.value?.diagnostics, serial: state.value ? { txDiscarded: state.value.serial.txDiscarded, rxDiscarded: state.value.serial.rxDiscarded } : undefined }, null, 2))
const bytes = (n: number) => n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MiB` : `${n} B`
const transferRate = (n: number | null) => n === null ? '계산 중…' : n >= 1048576 ? `${(n / 1048576).toFixed(2)} MiB/s` : `${(n / 1024).toFixed(1)} KiB/s`
function transferTime(n: number | null): string {
  if (n === null) return '계산 중…'
  const seconds = Math.ceil(n), hours = Math.floor(seconds / 3600), minutes = Math.floor(seconds % 3600 / 60)
  return hours ? `${hours}시간 ${minutes}분` : minutes ? `${minutes}분 ${seconds % 60}초` : `${seconds}초`
}
function save(): void { localStorage.setItem(savedKey, JSON.stringify({ config: config.value, rules: rules.value, wol: wol.value })) }
async function refresh(): Promise<void> {
  if (polling.value || disposed) return
  polling.value = true
  try { const next = await api.state(uid); if (!disposed) state.value = next }
  catch (e) { if (!disposed) error.value = String(e) }
  finally { polling.value = false }
}
async function run(action: () => Promise<void>, success = ''): Promise<void> {
  if (busy.value || disposed) return
  busy.value = true; error.value = ''; message.value = ''
  try { await action(); if (!disposed) { if (success) message.value = success; await refresh() } }
  catch (e) { if (!disposed) error.value = String(e) }
  finally { busy.value = false }
}
function confirm(title: string, text: string): Promise<boolean> { return new Promise(resolve => $q.dialog({ title, message: text, cancel: true, persistent: true }).onOk(() => resolve(true)).onCancel(() => resolve(false)).onDismiss(() => resolve(false))) }
async function chord(request: KeyChordRequest, ask = false): Promise<void> {
  if (!canType.value) return
  if (ask && !await confirm('타깃에 특수 입력 전송', `선택 채널 ${uidHex(uid)}에 이 키 조합을 전송할까요?`)) return
  const chosen = { ...request }; modifiers.value = 0
  await run(async () => { await props.beforeInput(); if (!disposed) await api.chord(uid, chosen) })
}
async function inputRelease(): Promise<void> { await run(async () => { await props.beforeInput(); await api.release(uid) }) }
async function storage(action: MiniStorageAction, ask = false): Promise<void> {
  if (ask && !await confirm('선택 채널 저장소 변경', 'SD 이미지/공유 상태를 변경합니다. 연결된 OS의 쓰기와 안전 제거를 먼저 마쳐 주세요. 계속할까요?')) return
  await run(async () => { await props.beforeInput(); await api.storage(uid, action) }, '저장소 작업 완료 · 타깃 OS의 인식 여부는 별도로 확인하세요.')
  const disk = state.value?.storage.disk
  const images = state.value?.storage.images ?? []
  if (action.kind === 'disk-create' || action.kind === 'disk-upload' || action.kind === 'folder-share') {
    const image = images.find(i => i.kind === 'disk' && i.id === disk?.id); if (image) selectImage(image)
  }
  if (!selectedImage.value && images[0]) selectImage(images[0])
}
async function formatImage(): Promise<void> {
  const image = selectedImage.value
  if (image?.kind !== 'disk') return
  if (await confirm('이미지 파일 포맷', `${image.name}의 내용을 지우고 ${formatName(diskFormat.value)}로 다시 만듭니다. 계속할까요?`)) await storage({ kind: 'disk-format', id: image.id, format: diskFormat.value, confirmed: true })
}
function createImage(format: 1 | 2 | 3): void {
  $q.dialog({ title: `${formatName(format)} 이미지 만들기`, message: '용량 (MiB, 64..32768)', prompt: { model: '256', type: 'number' }, cancel: true }).onOk((value: string) => {
    const sizeMiB = Number(value)
    if (!Number.isInteger(sizeMiB) || sizeMiB < 64 || sizeMiB > 32768) { error.value = '이미지 용량은 64..32768 MiB입니다.'; return }
    void storage({ kind: 'disk-create', format, sizeMiB })
  })
}
async function openImage(): Promise<void> {
  const image = selectedImage.value
  if (image?.kind === 'disk') await storage({ kind: 'disk-open', id: image.id, path: diskPath.value, writable: writable.value })
}
async function cancelStorage(): Promise<void> { try { await api.storage(uid, { kind: 'cancel' }); await refresh() } catch (e) { error.value = String(e) } }
async function readSector(): Promise<void> { await storage({ kind: 'disk-read', lba: lba.value }); sectorHex.value = state.value?.storage.sectorHex ?? '' }
async function writeSector(): Promise<void> { if (await confirm('섹터 직접 덮어쓰기', `선택 디스크 LBA ${lba.value}의 512바이트를 덮어씁니다. 계속할까요?`)) await storage({ kind: 'disk-write', lba: lba.value, hex: sectorHex.value, confirmed: true }) }
async function readUser(): Promise<void> { await storage({ kind: 'card-user-read' }); userText.value = state.value?.storage.userText ?? '' }
async function formatCard(format: 1 | 2): Promise<void> { if (await confirm('microSD 전체 삭제', `채널 ${uidHex(uid)}의 microSD에 있는 모든 데이터를 삭제합니다. 수락된 포맷은 취소할 수 없습니다. 진행할까요?`)) await storage({ kind: 'card-format', format, confirmed: true }) }
async function serialSend(): Promise<void> {
  await run(async () => {
    let data: Uint8Array
    if (serialHex.value) { const value = serialText.value.replace(/\s/g, ''); if (!value || value.length % 2 || !/^[0-9a-f]+$/i.test(value)) throw new Error('올바른 HEX 바이트가 필요합니다.'); data = Uint8Array.from(value.match(/../g)!.map(v => parseInt(v, 16))) }
    else data = new TextEncoder().encode(serialText.value + (newline.value ? '\r\n' : ''))
    await api.serialWrite(uid, Array.from(data)); serialText.value = ''
  })
}
async function saveNetwork(): Promise<void> { await run(async () => { await api.configure(uid, config.value); save() }, '네트워크 설정을 저장했습니다.') }
function addRule(): void { rules.value.push({ id: crypto.randomUUID(), port: 0, protocol: 'tcp', listenPort: 2222, targetIp: config.value.serverIp.replace(/\.1$/, '.100'), targetPort: 22 }) }
async function startForward(): Promise<void> { await run(async () => { await api.forward(uid, rules.value); save() }) }
async function wake(): Promise<void> { if (!await confirm('WOL 전송', `선택 채널에 저장한 MAC ${wol.value.mac}으로 WOL 패킷을 보낼까요?`)) return; await run(async () => { save(); const result = await window.kbmNative.wake.wol(0, wol.value); message.value = wakeResultText(result.code) }) }
function blur(): void { void api.deactivate(uid).catch(() => {}) }
onMounted(() => {
  void refresh().then(async () => { if (!disposed && !configured) { configured = true; try { await api.configure(uid, config.value) } catch (e) { error.value = String(e) } } })
  void window.kbmNative.wake.adapters().then(value => { if (!disposed) adapters.value = value }).catch(e => { error.value = String(e) })
  timer = setInterval(() => void refresh(), 1000); window.addEventListener('blur', blur)
})
onBeforeUnmount(() => { disposed = true; if (timer) clearInterval(timer); window.removeEventListener('blur', blur); blur() })
</script>

<style scoped lang="scss">
.channel-tools { border: 1px solid #283750; border-radius: 10px; background: #0c1420; }
.keyboard-row { display: flex; gap: 4px; margin: 4px 0; min-width: 680px; .q-btn { min-width: 0; font-size: 11px; padding: 5px 2px; } }
.q-tab-panel { overflow-x: auto; } .terminal { min-height: 200px; max-height: 340px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; background: #03060a; padding: 12px; }
.mono, pre { font-family: Consolas, monospace; } .q-input { min-width: 100px; } p { font-size: 12px; line-height: 1.8; }
.tools-card { background: #0c1420ed; color: #e2eaf4; }
.console-tools { position: absolute; bottom: 92px; left: 38px; right: 38px; z-index: 3; pointer-events: none; }
.console-tools > *, .console-tools .tools-card { pointer-events: auto; }
.console-tools .tools-card { border: 1px solid #30445a; border-radius: 8px; max-width: 1280px; margin: 0 auto; }
.console-tools :deep(.q-tab-panel) { padding: 4px 12px 10px; }
.console-tools .keyboard-row .q-btn { padding: 3px 2px; }
.modal-card { width: 1120px; max-width: 95vw; max-height: 90vh; overflow: auto; }
.storage-layout { display: grid; grid-template-columns: minmax(0, 1.35fr) minmax(280px, 1fr); gap: 24px; align-items: start; }
.storage-functions h3, .storage-images h3 { font-size: 17px; line-height: 1.5; margin: 0 0 12px; }
.storage-images { position: sticky; top: 0; border-left: 1px solid #30445a; padding-left: 20px; }
.storage-images .q-list { max-height: 580px; overflow-y: auto; } .selected-image { background: #173d47; color: #85edf1; }
@media (max-width: 800px) { .storage-layout { grid-template-columns: 1fr; } .storage-images { position: static; border-left: 0; padding-left: 0; } }
</style>
