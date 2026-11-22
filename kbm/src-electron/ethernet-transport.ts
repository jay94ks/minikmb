import { EthernetAssembler, fragmentFrame } from '../src/services/ethernet'
import { Op, type Packet } from '../src/services/protocol'
import type { NetworkPort } from '../src/shared/network'
import type { RoutedTransport } from './routed-transport'

// Native Ethernet framing uses the same UART arbiter as disk/HID. No renderer
// IPC round trip per64-byte fragment. Preserve small fragments for HID latency.
export class EthernetTransport {
  private assemblers = [new EthernetAssembler(), new EthernetAssembler()]
  private epochs = [0, 0]
  private frameId = 0
  constructor(private transport: RoutedTransport,
    private receive: (port: NetworkPort, frame: Uint8Array) => void) {}

  async send(port: NetworkPort, frame: Uint8Array): Promise<void> {
    const epoch = this.epochs[port]
    const current = (): boolean => epoch === this.epochs[port]
    for (const payload of fragmentFrame(frame, ++this.frameId)) {
      if (!current()) throw new Error('Ethernet transfer canceled')
      const response = await this.transport.request({ op: Op.Route,
        payload: Uint8Array.of(port, 0, 0, Op.NetWrite, ...payload) }, true, current)
      if (response.payload[3] !== 0 || response.payload[5] !== 0) {
        throw new Error(`UART${port} Ethernet fragment rejected`)
      }
    }
  }

  accept(packet: Packet): boolean {
    if (packet.op !== 0xf6) return false
    const port = packet.payload[0]
    if ((port === 0 || port === 1) && packet.payload.length > 11) {
      const frame = this.assemblers[port]!.accept(packet.payload.subarray(1))
      if (frame) this.receive(port, frame)
    }
    return true
  }

  cancel(port: NetworkPort): void {
    ++this.epochs[port]!
    this.assemblers[port] = new EthernetAssembler()
  }
  reset(): void { this.cancel(0); this.cancel(1) }
}
