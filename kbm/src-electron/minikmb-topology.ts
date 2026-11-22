import type { MiniUsbNode, MiniBoard, MiniChannel } from '../src/shared/minikmb'
const lower = (s: string) => s.toLowerCase()
const hub = (n: MiniUsbNode) => /vid_05e3&pid_06(10|26)/i.test(n.id)
// MiniKMB.kicad_sch U10 GL3510: U9=P3/U3=P1, U12=P4/U4=P2.
// Verified from the current schematic export work/minikmb-current-netlist.xml.
export const miniPortPairs: Readonly<Record<number, { slot: number; capturePort: number }>> = {
  3: { slot: 0, capturePort: 1 }, 4: { slot: 1, capturePort: 2 }
}
function branch(n: MiniUsbNode): string {
  // USB2 and USB3 companion trees may use different root controllers/ports.
  // Require matching downstream port suffix AND container, never container alone.
  const ports = n.location.find(v => v.startsWith('PCIROOT'))?.match(/#USB\(\d+\)/g) ?? []
  return ports.slice(1).join('')
}
export function groupMiniBoards(nodes: MiniUsbNode[], channels: MiniChannel[]): MiniBoard[] {
  const byId = new Map(nodes.map(n => [lower(n.id), n]))
  const boards = new Map<string, MiniBoard>()
  for (const channel of channels) {
    const node = nodes.find(n => /vid_cafe&pid_400[124]\\/i.test(n.id) && n.id.split('\\').at(-1)?.toUpperCase() === channel.uid)
    const parent = node ? byId.get(lower(node.parent)) : undefined
    const id = parent && hub(parent) ? lower(parent.id) : `mcu:${channel.uid}`
    channel.boardId = id; channel.usbPort = node?.address ?? 0
    let board = boards.get(id)
    if (!board) { board = { id, label: `MiniKMB · 0x${channel.uid.slice(0, 8)}…${channel.uid.slice(-4)}`, channels: [], captures: [], pairing: '' }; boards.set(id, board) }
    board.channels.push(channel)
    if (parent && parent.container && !/^\{?00000000-/i.test(parent.container) && !board.captures.length) {
      const peers = nodes.filter(n => hub(n) && lower(n.container) === lower(parent.container) && branch(n) === branch(parent))
      const usb3 = peers.filter(n => /pid_0626/i.test(n.id))
      const usb2 = peers.filter(n => /pid_0610/i.test(n.id))
      // Multiple identical hubs at the same suffix are ambiguous; do not merge boards.
      if (usb3.length === 1 && usb2.length === 1 && usb2[0]!.id.toLowerCase() === parent.id.toLowerCase()) {
        board.captures = nodes.filter(n => /vid_(345f|534d)&pid_2130\\/i.test(n.id) && lower(n.parent) === lower(usb3[0]!.id))
          .map(n => ({ id: n.id, name: n.name, port: n.address })).sort((a, b) => a.port - b.port)
      } else board.pairing = 'USB2/USB3 허브 대응을 확정할 수 없습니다. 다른 보드에 자동 연결하지 않습니다.'
    }
  }
  for (const board of boards.values()) {
    for (const channel of board.channels) {
      const pair = miniPortPairs[channel.usbPort]
      const capture = pair && board.captures.filter(c => c.port === pair.capturePort)
      channel.slot = pair?.slot ?? null
      channel.captureId = capture?.length === 1 ? capture[0]!.id : null
      channel.pairingError = channel.captureId ? '' : board.pairing || (pair ? `허브 P${pair.capturePort}의 MS2130이 없습니다.` : '회로도의 MCU 포트 P3/P4와 일치하지 않습니다.')
    }
    board.channels.sort((a, b) => a.usbPort - b.usbPort || a.uid.localeCompare(b.uid))
  }
  return [...boards.values()].sort((a, b) => a.id.localeCompare(b.id))
}
