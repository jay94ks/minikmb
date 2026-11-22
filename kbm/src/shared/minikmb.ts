export interface MiniUsbNode {
  id: string; parent: string; container: string; location: string[]; address: number; name: string
}
export interface MiniCapture { id: string; name: string; port: number }
export interface MiniChannel {
  name?: string; nameSupported?: boolean; nameError?: string
  uid: string; boardId: string; port: string; usbPort: number
  connected: boolean; features: number; targetState: number; error: string
  slot: number | null; captureId: string | null; pairingError: string
}
export function channelName(value: unknown): string {
  if (typeof value !== 'string') throw new Error('채널 이름은 문자열이어야 합니다.')
  const name = value.trim().normalize('NFC')
  if (/[\u0000-\u001f\u007f-\u009f\u2028\u2029]/u.test(name) || new TextEncoder().encode(name).length > 64 || /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(name)) throw new Error('채널 이름은 제어문자 없이 UTF-8 64바이트까지 입력하세요.')
  return name
}
export interface MiniBoard { id: string; label: string; channels: MiniChannel[]; captures: MiniCapture[]; pairing: string }
export interface MiniInventory { boards: MiniBoard[]; error: string; scannedAt: number }
export interface MiniKeyboardState { uid: string; active: boolean; error: string }
export type MiniInput =
  | { kind: 'key'; down: boolean; usage: number }
  | { kind: 'button'; down: boolean; button: number }
  | { kind: 'move'; x: number; y: number }
  | { kind: 'wheel'; vertical: number; horizontal: number }
  | { kind: 'release' }
export function validUid(value: unknown): value is string { return typeof value === 'string' && /^[0-9A-F]{24}$/.test(value) }
export function uidHex(uid: string): string { return validUid(uid) ? `0x${uid}` : '—' }
export function inputPacket(input: MiniInput): { opcode: number; payload: Uint8Array } {
  if (!input || typeof input !== 'object') throw new Error('입력 형식 오류')
  const u16 = (...values: number[]) => {
    if (values.some(v => !Number.isInteger(v) || v < 0 || v > 65535)) throw new Error('입력 범위 오류')
    const b = new Uint8Array(values.length * 2), d = new DataView(b.buffer)
    values.forEach((v, i) => d.setUint16(i * 2, v, true)); return b
  }
  switch (input.kind) {
    case 'release': return { opcode: 0x107, payload: new Uint8Array() }
    case 'key':
      if (typeof input.down !== 'boolean' || !Number.isInteger(input.usage) ||
          !((input.usage >= 4 && input.usage <= 0xa4) || (input.usage >= 0xe0 && input.usage <= 0xe7))) throw new Error('지원하지 않는 키')
      return { opcode: input.down ? 0x101 : 0x102, payload: u16(input.usage) }
    case 'button':
      if (typeof input.down !== 'boolean' || !Number.isInteger(input.button) || input.button < 1 || input.button > 5) throw new Error('버튼 범위 오류')
      return { opcode: input.down ? 0x104 : 0x105, payload: Uint8Array.of(input.button) }
    case 'move':
      if (input.x > 32767 || input.y > 32767) throw new Error('좌표 범위 오류')
      return { opcode: 0x103, payload: u16(input.x, input.y) }
    case 'wheel': {
      if ([input.vertical, input.horizontal].some(v => !Number.isInteger(v) || v < -127 || v > 127)) throw new Error('휠 범위 오류')
      const b = new Uint8Array(4), d = new DataView(b.buffer)
      d.setInt16(0, input.vertical, true); d.setInt16(2, input.horizontal, true)
      return { opcode: 0x106, payload: b }
    }
    default: throw new Error('지원하지 않는 입력')
  }
}
