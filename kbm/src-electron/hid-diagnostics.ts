import { decodeHidDiagnostics, type HidAlarm, type HidDiagnosticsState } from '../src/shared/hid-diagnostics'
import type { Packet } from '../src/services/protocol'
import type { OperationLog } from '../src/shared/logging'
export class HidDiagnostics {
  private ports: HidDiagnosticsState = [this.empty(), this.empty()]
  private connected = false
  private since = [0,0]
  private usbSince = [0,0]
  constructor(private log: OperationLog = () => {}, private publish: (alarm: HidAlarm) => void = () => {}, private now = () => Date.now()) {}
  private empty() { return { online: false, seenAt: 0, bootCount: 0, alarms: [] as HidAlarm[] } }
  state(): HidDiagnosticsState { return structuredClone(this.ports) }
  master(connected: boolean): void {
    if (this.connected === connected) return
    this.connected = connected
    this.since = [this.now(),this.now()]
    if (!connected) for (const port of [0,1] as const) { this.ports[port].online = false; this.ports[port].snapshot = undefined; this.ports[port].seenAt = 0; this.ports[port].alarms = [] }
  }
  online(port: 0 | 1, value: boolean): void {
    if (this.ports[port].online !== value) this.since[port] = this.now()
    this.ports[port].online = value
  }
  boot(port: 0 | 1, firmware: number): void {
    const state = this.ports[port]
    state.bootCount++; state.firmware = firmware; state.snapshot = undefined; state.seenAt = 0
    this.online(port,false); this.since[port] = this.now()
    this.log('warn','hid.slave_boot',{port,firmware,bootCount:state.bootCount,previousRequestsCanceled:true,hostActionConfirmed:false})
    this.publish({port,code:'slave-boot',level:'info',message:'slave 새 부팅 감지 · 이전 요청 정리 중'})
  }
  accept(packet: Packet): boolean {
    const value = decodeHidDiagnostics(packet)
    if (!value) return false
    const { port, snapshot } = value, previous = this.ports[port].snapshot
    if (!previous || (previous.flags & 7) !== (snapshot.flags & 7)) this.usbSince[port] = this.now()
    this.ports[port].snapshot = snapshot; this.ports[port].seenAt = this.now()
    const changed = !previous || (previous.flags & 23) !== (snapshot.flags & 23) || previous.busResets !== snapshot.busResets
    this.log(changed ? 'info' : 'debug','hid.snapshot',{port,...snapshot})
    if (previous && snapshot.queueRejected !== previous.queueRejected) this.log('warn','hid.queue_rejected',{port,count:(snapshot.queueRejected-previous.queueRejected)>>>0})
    if (previous && snapshot.rxErrors !== previous.rxErrors) this.log('warn','hid.uart_rx_errors',{port,count:(snapshot.rxErrors-previous.rxErrors)>>>0})
    this.tick(); return true
  }
  tick(): void {
    if (!this.connected) return
    for (const port of [0,1] as const) {
      const state = this.ports[port], s = state.snapshot, age = this.now() - this.since[port]!
      const alarms: HidAlarm[] = []
      const add = (code: string, level: HidAlarm['level'], message: string): void => { alarms.push({port,code,level,message}) }
      if (!state.online && age >= 1000) add('uart-offline','error','UART 응답 없음 · USB 인식 여부와 별개')
      else if (state.online) {
        if ((!state.seenAt || this.now()-state.seenAt>3500) && age>=4000) add('diagnostics-stale','warn','HID 상태 보고 지연 또는 미지원 펌웨어')
        if (s && this.now()-state.seenAt<=3500) {
          if (s.suspended) add('usb-suspended','info','USB 절전/버스 유휴 · 물리 분리 여부 미확인')
          else if (!s.mounted && this.now()-this.usbSince[port]!>=5000) add('usb-unconfigured','warn','USB 설정 대기 · BIOS/호스트 초기화 확인 필요')
          else if (s.mounted && !s.ready && this.now()-this.usbSince[port]!>=5000) add('usb-not-ready','warn','USB 설정됨 · 사용 준비 안 됨')
          if (s.ready && !s.suspended && s.stable && s.waiting && s.waitMs>=2000) add('hid-stalled','error','USB 설정됨 · HID 전송 완료 정체')
          if (s.queue>=28) add('hid-queue-pressure','warn','HID 큐 포화 임박 · 입력 소비 지연')
        }
      }
      this.replace(port,alarms)
    }
  }
  private replace(port: 0 | 1, alarms: HidAlarm[]): void {
    const old = this.ports[port].alarms
    for (const alarm of alarms) if (!old.some(item=>item.code===alarm.code)) { this.log(alarm.level,'hid.alarm',{port,code:alarm.code,message:alarm.message}); this.publish(alarm) }
    for (const alarm of old) if (!alarms.some(item=>item.code===alarm.code)) {
      if (alarm.code === 'uart-offline' && !this.ports[port].online) { alarms.push(alarm); continue }
      const state = this.ports[port], s = state.snapshot
      const ready = state.online && Boolean(s?.ready && !s.suspended) && this.now()-state.seenAt<=3500
      this.log('info',ready?'hid.recovered':'hid.alarm_cleared',{port,code:alarm.code,usbReadyConfirmed:ready,hostActionConfirmed:false})
      this.publish({...alarm,level:'info',message:`${ready?'USB 보고 상태 복귀':'조건 해제 · 정상 여부 미확인'}: ${alarm.message}`,recovered:true})
    }
    this.ports[port].alarms = alarms
  }
}
