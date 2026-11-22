export const STX = 0x02
export const ETX = 0x03
export const ESC = 0x1b
export const MAX_FRAME_PAYLOAD = 4118

export const Op = {
  Ping: 0x00,
  KeyDown: 0x10,
  KeyUp: 0x11,
  SystemDown: 0x12,
  SystemUp: 0x13,
  MouseMove: 0x20,
  MouseDown: 0x21,
  MouseUp: 0x22,
  SerialWrite: 0x30,
  SerialStatus: 0x31,
  NetWrite: 0x40,
  NetStatus: 0x41,
  NetEnable: 0x43,
  DiskConfig: 0x44,
  DiskData: 0x45,
  DiskStatus: 0x46,
  DiskFail: 0x47,
  TempBaud: 0x50,
  Echo: 0x51,
  RebootSlave: 0x60,
  WakePc: 0x61,
  HidStatus: 0x63,
  Status: 0x71,
  Route: 0x73,
  NativeMode: 0x77,
  ReleaseAll: 0x7f,
  OnlineChanged: 0xf2,
  SerialData: 0xf4,
  SerialState: 0xf5,
  NetData: 0xf6,
  DiskRead: 0xf8,
  SlaveBoot: 0xf9,
  HidDiagnostics: 0xfa
} as const

export interface Packet {
  op: number
  payload: Uint8Array
}

export type DecodeEvent =
  | { type: 'packet'; packet: Packet }
  | { type: 'error'; error: 'escape' | 'etx' | 'checksum' | 'length'; requestOp: number }

type DecoderState =
  | 'wait-stx'
  | 'op'
  | 'len'
  | 'len-high'
  | 'payload'
  | 'payload-escape'
  | 'etx'
  | 'checksum'

function isControl(byte: number): boolean {
  return byte === STX || byte === ETX || byte === ESC
}

export function checksum(packet: Packet): number {
  let sum = (packet.op + (packet.payload.length & 255) + (packet.payload.length >>> 8)) & 0xff
  for (const byte of packet.payload) sum = (sum + byte) & 0xff
  return sum
}

export function encodePacket(packet: Packet): Uint8Array {
  if (packet.payload.length > MAX_FRAME_PAYLOAD) {
    throw new RangeError('Payload cannot exceed 4118 bytes')
  }

  const frame: number[] = [STX, packet.op & 0xff, packet.payload.length & 255, packet.payload.length >>> 8]
  for (const byte of packet.payload) {
    if (isControl(byte)) frame.push(ESC)
    frame.push(byte)
  }
  frame.push(ETX, checksum(packet))
  return Uint8Array.from(frame)
}

export class ProgressiveDecoder {
  private state: DecoderState = 'wait-stx'
  private op = 0
  private length = 0
  private sum = 0
  private payload: number[] = []

  feed(chunk: Uint8Array): DecodeEvent[] {
    const events: DecodeEvent[] = []
    for (const byte of chunk) {
      const event = this.feedByte(byte)
      if (event) events.push(event)
    }
    return events
  }

  private restartAtStx(): void {
    this.state = 'op'
    this.payload = []
    this.sum = 0
  }

  private resetAfterError(byte: number): void {
    if (byte === STX) this.restartAtStx()
    else {
      this.state = 'wait-stx'
      this.payload = []
      this.sum = 0
    }
  }

  private feedByte(byte: number): DecodeEvent | undefined {
    switch (this.state) {
      case 'wait-stx':
        if (byte === STX) this.restartAtStx()
        return
      case 'op':
        this.op = byte
        this.sum = byte
        this.state = 'len'
        return
      case 'len':
        this.length = byte
        this.sum = (this.sum + byte) & 0xff
        this.state = 'len-high'
        return
      case 'len-high':
        this.length |= byte << 8
        this.sum = (this.sum + byte) & 0xff
        this.payload = []
        if (this.length > MAX_FRAME_PAYLOAD) {
          this.state = 'wait-stx'
          return { type: 'error', error: 'length', requestOp: this.op }
        }
        this.state = this.length === 0 ? 'etx' : 'payload'
        return
      case 'payload':
        if (byte === ESC) {
          this.state = 'payload-escape'
          return
        }
        if (byte === STX || byte === ETX) {
          this.resetAfterError(byte)
          return { type: 'error', error: 'escape', requestOp: this.op }
        }
        this.pushPayload(byte)
        return
      case 'payload-escape':
        if (!isControl(byte)) {
          this.resetAfterError(byte)
          return { type: 'error', error: 'escape', requestOp: this.op }
        }
        this.pushPayload(byte)
        return
      case 'etx':
        if (byte !== ETX) {
          this.resetAfterError(byte)
          return { type: 'error', error: 'etx', requestOp: this.op }
        }
        this.state = 'checksum'
        return
      case 'checksum': {
        const packet = { op: this.op, payload: Uint8Array.from(this.payload) }
        const valid = byte === this.sum
        this.state = 'wait-stx'
        this.payload = []
        return valid
          ? { type: 'packet', packet }
          : { type: 'error', error: 'checksum', requestOp: this.op }
      }
    }
  }

  private pushPayload(byte: number): void {
    this.payload.push(byte)
    this.sum = (this.sum + byte) & 0xff
    this.state = this.payload.length === this.length ? 'etx' : 'payload'
  }
}

export function u16le(value: number): [number, number] {
  const clamped = Math.max(0, Math.min(0x7fff, Math.round(value)))
  return [clamped & 0xff, (clamped >>> 8) & 0xff]
}

