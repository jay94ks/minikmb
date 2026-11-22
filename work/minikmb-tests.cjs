// Application tests only: no physical serial, HID, camera, SD or NIC operations.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict')
const root = path.resolve(__dirname, '..'), ts = require('../kbm/node_modules/typescript'), cache = new Map()
function load(file) {
  const full = path.resolve(root, file)
  if (cache.has(full)) return cache.get(full)
  const exports = {}; cache.set(full, exports)
  const js = ts.transpileModule(fs.readFileSync(full, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
  vm.runInThisContext(`(function(exports,require){${js}\n})`, { filename: full })(exports, name => name === 'serialport' ? { SerialPort: { list: async () => [] } } : name.startsWith('.') ? load(path.relative(root, path.resolve(path.dirname(full), name + '.ts'))) : require(name))
  return exports
}
const { miniEncode, MiniDecoder } = load('kbm/src/services/minikmb-protocol.ts')
const { MiniConnection } = load('kbm/src-electron/minikmb-connection.ts')
const { SingleCapture } = load('kbm/src/services/single-capture.ts')
const { groupMiniBoards } = load('kbm/src-electron/minikmb-topology.ts')
const { inputPacket } = load('kbm/src/shared/minikmb.ts')
const UID = '240043001451333532363439'
async function main() {
  const { AbsolutePointer, absolutePoint } = load('kbm/src/services/absolute-pointer.ts')
  const rect = { left: 10, top: 20, width: 1000, height: 1000 }
  assert.equal(absolutePoint(rect, 1920, 1080, 510, 100), undefined, 'letterbox is not a target position')
  assert.deepEqual(absolutePoint(rect, 1920, 1080, 510, 520), { x: 16384, y: 16384 })
  assert.deepEqual(absolutePoint(rect, 1920, 1080, 1010, 801.25), { x: 32767, y: 32767 })
  assert.deepEqual(absolutePoint(rect, 1920, 1080, 9999, -10, true), { x: 32767, y: 0 })
  assert.equal(absolutePoint({ ...rect, width: 0 }, 1920, 1080, 10, 20), undefined)
  const mouse = [], mouseErrors = [], aPoint = { x: 0, y: 0 }, bPoint = { x: 32767, y: 32767 }
  let unblock
  const pointer = new AbsolutePointer(async input => { mouse.push(input); if (mouse.length === 1) await new Promise(r => { unblock = r }) }, e => mouseErrors.push(e))
  pointer.button(aPoint, 1, true)
  await new Promise(r => setImmediate(r))
  pointer.move({ x: 1, y: 2 }); pointer.move({ x: 3, y: 4 })
  pointer.button(bPoint, 1, false); pointer.button(aPoint, 1, true); pointer.button(aPoint, 1, false)
  unblock(); await pointer.idle()
  assert.deepEqual(mouse.map(i => i.kind === 'button' ? `button:${i.down}` : `move:${i.x},${i.y}`), [
    'move:0,0', 'button:true', 'move:3,4', 'move:32767,32767', 'button:false', 'move:0,0', 'button:true', 'move:0,0', 'button:false'
  ], 'rapid second click cannot overtake the first UP')
  const canceled = []
  let finishMove
  const switched = new AbsolutePointer(async input => { canceled.push(input); await new Promise(r => { finishMove = r }) }, e => mouseErrors.push(e))
  switched.button(aPoint, 1, true); await new Promise(r => setImmediate(r)); switched.cancel(); finishMove(); await switched.idle()
  assert.equal(canceled.length, 1, 'cancel between MOVE and DOWN must suppress the old DOWN')
  assert.equal(mouseErrors.length, 0)
  console.log('PASS absolute video coordinates/letterboxing, drag bounds, move coalescing, rapid click DOWN/UP ordering and canceled transition safety')
  const { miniTopologyScript } = load('kbm/src-electron/minikmb-service.ts')
  assert(miniTopologyScript.includes('^USB\\\\VID_'), 'C# regex must receive two literal backslashes')
  assert(!miniTopologyScript.includes('Get-PnpDevice'), 'Discovery must not depend on slow PnP/WMI providers')
  for (const length of [0, 1, 64, 254, 255, 1008, 1024]) {
    const frame = { kind: 1, channel: 4, opcode: 0x402, sequence: 71, session: 0xfedcba9876543210n, payload: Uint8Array.from({ length }, (_, i) => i & 255) }
    const bytes = miniEncode(frame), decoder = new MiniDecoder(), frames = []
    for (const byte of bytes) frames.push(...decoder.feed(Uint8Array.of(byte)))
    assert.deepEqual(frames, [frame])
    const bad = bytes.slice(); bad[4] ^= 0x40
    assert.equal(new MiniDecoder().feed(bad).length, 0)
    const recover = new MiniDecoder()
    recover.feed(new Uint8Array(4096).fill(1)); recover.feed(Uint8Array.of(0))
    assert.deepEqual(recover.feed(bytes), [frame])
  }
  assert.throws(() => inputPacket({ kind: 'move', x: -1, y: 0 }))
  assert.throws(() => inputPacket({ kind: 'button', down: true, button: 8 }))
  assert.deepEqual([...inputPacket({ kind: 'wheel', vertical: -1, horizontal: 1 }).payload], [255, 255, 1, 0])
  console.log('PASS firmware-compatible COBS/CRC, uint64 session, every-byte splits, bounds and corruption recovery')

  const ch = uid => ({ uid, port: 'COM-not-identity', connected: true, features: 1, targetState: 1, usbPort: 0, boardId: '', error: '' })
  const node = (id, parent, address, branch, container = 'same-generic-container') => ({ id, parent, address, container, name: 'USB3 Video', location: ['PCIROOT(0)#USBROOT(0)#USB(5)' + branch] })
  const nodes = [], channelList = []
  for (let b = 1; b <= 2; b++) {
    const u2 = `USB\\VID_05E3&PID_0610\\BOARD${b}`, u3 = `USB\\VID_05E3&PID_0626\\BOARD${b}`
    nodes.push(node(u2, 'root', b, `#USB(${b})`), node(u3, 'root', b, `#USB(${b})`))
    for (let c = 0; c < 2; c++) {
      const uid = String(b).repeat(23) + c; channelList.push(ch(uid))
      nodes.push(node(`USB\\VID_CAFE&PID_4004\\${uid}`, u2, c + 3, ''), node(`USB\\VID_345F&PID_2130\\${b}-${c}`, u3, c + 1, ''))
    }
  }
  const boards = groupMiniBoards(nodes, channelList)
  assert.equal(boards.length, 2)
  assert(boards.every(b => b.channels.length === 2 && b.captures.length === 2))
  assert(boards.every(b => b.channels.every(c => c.captureId === b.captures[c.usbPort - 3].id && c.slot === c.usbPort - 3)))
  nodes.push(node('USB\\VID_05E3&PID_0626\\AMBIGUOUS', 'root', 1, '#USB(1)'))
  assert.equal(groupMiniBoards(nodes, channelList)[0].captures.length, 0)
  console.log('PASS multiple boards, generic duplicate container IDs, topology ambiguity never silently paired')

  let live = 0, peak = 0, finishOpen, started = []
  const owner = new SingleCapture(async id => {
    started.push(id); live++; peak = Math.max(peak, live)
    if (id === 'A') await new Promise(resolve => { finishOpen = resolve })
    return id
  }, () => live--)
  const a = owner.select('A'); await new Promise(resolve => setImmediate(resolve))
  const b = owner.select('B'), c = owner.select('C'); finishOpen()
  assert.equal(await a, undefined); assert.equal(await b, undefined); assert.equal(await c, 'C')
  await owner.select(); assert.equal(live, 0); assert.equal(peak, 1); assert.deepEqual(started, ['A', 'C'])
  console.log('PASS one-stream invariant including delayed opens, rapid switches, stale completion and shutdown')

  const sent = [], decode = new MiniDecoder(), hello = new Uint8Array(40)
  let client, ticket = 0, drop = false, closed = 0, duplicateBusy = false
  const session = 0xfedcba9876543210n
  function response(request, result, status = 0) {
    const payload = new Uint8Array(8 + result.length)
    new DataView(payload.buffer).setUint16(0, status, true); payload.set(result, 8)
    client.feed(miniEncode({ ...request, kind: 2, session, payload }))
  }
  client = new MiniConnection('', {
    close: () => closed++,
    write: async bytes => {
      const [request] = decode.feed(bytes); assert(request); sent.push(request)
      if (drop) return
      queueMicrotask(() => {
        if (request.opcode === 1) {
          hello.set(request.payload.slice(4), 0); hello.set(Buffer.from(UID, 'hex'), 8)
          new DataView(hello.buffer).setUint32(28, 1, true); response(request, hello)
        } else if (request.opcode === 3) response(request, hello)
        else if (request.channel === 1) {
          if (duplicateBusy && request.opcode === 0x107) { duplicateBusy = false; response(request, new Uint8Array(), 4); return }
          const done = new Uint8Array(8), result = new Uint8Array(4)
          new DataView(result.buffer).setUint32(0, ++ticket, true); new DataView(done.buffer).setUint32(0, ticket, true)
          // A stale reply must not consume this command; DONE may precede ACK parsing.
          client.feed(miniEncode({ ...request, kind: 2, session, sequence: request.sequence + 99, payload: new Uint8Array(8) }))
          client.feed(miniEncode({ ...request, kind: 3, opcode: 0x180, session, payload: done }))
          response(request, result)
        } else response(request, new Uint8Array(64))
      })
    }
  }, () => {})
  await client.hello(); assert.equal(client.features, 1); assert.equal(client.uid, UID)
  await Promise.all([client.command(0, 4), client.hid(0x101, Uint8Array.of(4, 0), () => true)])
  duplicateBusy = true; await client.hid(0x107, new Uint8Array(), () => true)
  assert.equal(sent.filter(r => r.opcode === 0x107).length, 2)
  await assert.rejects(client.hid(0x101, Uint8Array.of(5, 0), () => false))
  assert.equal(sent.filter(r => r.opcode === 0x101).length, 1)
  drop = true
  await assert.rejects(client.hid(0x101, Uint8Array.of(6, 0), () => true))
  assert.equal(sent.filter(r => r.opcode === 0x101).length, 2, 'lost DOWN ACK must never retry DOWN')
  assert.equal(closed, 1)
  console.log('PASS HELLO/INFO identity, independent channels, ACK matching, HID_DONE, release-only BUSY retry, canceled/lost DOWN safety')

  const { MiniService } = load('kbm/src-electron/minikmb-service.ts')
  const { channelName } = load('kbm/src/shared/minikmb.ts')
  assert.equal(channelName(' 서버 A '), '서버 A'); assert.equal(channelName(''), '')
  assert.throws(() => channelName('가'.repeat(22))); assert.throws(() => channelName('bad\nname'))
  const service = new MiniService(() => {}), other = '360041001451333532363439', operations = []
  let finishA, queuedCurrent
  const entry = uid => ({ state: { ...ch(uid), targetState: 1 }, pulse: false, fingerprint: '', client: {
    async hid(op, payload, current) {
      operations.push([uid, op]); queuedCurrent = current
      if (uid === UID && op === 0x107 && !finishA) await new Promise(resolve => { finishA = resolve })
    }, close() {}
  } })
  service.connections.set(UID, entry(UID)); service.connections.set(other, entry(other))
  await service.selectInput(UID)
  const selectB = service.selectInput(other); await new Promise(resolve => setImmediate(resolve))
  await service.selectInput(UID); finishA(); await assert.rejects(selectB)
  await assert.rejects(service.input(other, { kind: 'key', down: true, usage: 4 }))
  await service.input(UID, { kind: 'key', down: true, usage: 4 })
  const acceptedCurrent = queuedCurrent; assert(acceptedCurrent())
  await service.releaseInput(); assert(!acceptedCurrent())
  assert.equal(operations.at(-1)[0], UID); assert.equal(operations.at(-1)[1], 0x107)
  await service.input(UID, { kind: 'key', down: true, usage: 4 })
  await service.selectInput(other)
  await assert.rejects(service.input(UID, { kind: 'key', down: true, usage: 4 }))
  await service.input(other, { kind: 'key', down: true, usage: 4 })
  service.connections.get(other).state.features = 0
  await assert.rejects(service.input(other, { kind: 'key', down: true, usage: 4 }))
  await service.clearInput()
  await assert.rejects(service.input(other, { kind: 'key', down: true, usage: 4 }))
  service.connections.get(UID).client.command = async (channel, op) => { assert.equal(channel, 3); assert.equal(op, 0x31e); const n = Buffer.from('서버 A'), b = Buffer.alloc(4 + n.length); b.writeUInt16LE(n.length); n.copy(b, 4); return b }
  await service.refreshName(UID); assert.equal(service.connections.get(UID).state.name, '서버 A')
  service.connections.get(UID).client.command = async () => { throw Object.assign(Error('no card'), { status: 9 }) }
  await service.refreshName(UID); assert.equal(service.connections.get(UID).state.name, '', 'removed card clears the previous name')
  await service.stop()
  console.log('PASS direct selected-channel input, canceled selection races, automatic resume after release, old UID rejection and disconnect cleanup')
}
main().catch(error => { console.error(error); process.exitCode = 1 })
