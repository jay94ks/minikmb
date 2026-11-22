// No USB, HID, network sockets or physical disks: all MCU transports are fakes.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), os = require('node:os')
const assert = require('node:assert/strict'), { createRequire } = require('node:module'), { EventEmitter } = require('node:events'), { crc32 } = require('node:zlib')
const ts = require('../kbm/node_modules/typescript'), root = path.resolve(__dirname, '..'), cache = new Map()
let fakeSpawn
function load(relative) {
  const file = path.resolve(root, relative)
  if (cache.has(file)) return cache.get(file)
  const exports = {}, local = createRequire(file); cache.set(file, exports)
  const code = ts.transpileModule(fs.readFileSync(file, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
  vm.runInThisContext(`(function(exports,require){${code}\n})`, { filename: file })(exports, name => name === 'node:child_process' ? { spawn: (...args) => fakeSpawn(...args) } : name.startsWith('.') ? load(path.relative(root, path.resolve(path.dirname(file), name + '.ts'))) : local(name))
  return exports
}
const u32 = (...n) => { const b = Buffer.alloc(n.length * 4); n.forEach((v, i) => b.writeUInt32LE(v, i * 4)); return b }
const tick = () => new Promise(r => setTimeout(r, 0))
async function main() {
  const { reactive } = require('../kbm/node_modules/vue')
  let exposed, ipcCalls = []
  const preload = ts.transpileModule(fs.readFileSync(path.join(root, 'kbm/src-electron/electron-preload.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText
  vm.runInThisContext(`(function(exports,require){${preload}\n})`)({}, name => name === 'electron' ? {
    contextBridge: { exposeInMainWorld: (name, value) => { if (name === 'kbmNative') exposed = value } },
    ipcRenderer: { invoke: (...args) => { ipcCalls.push(structuredClone(args)); return Promise.resolve() }, on() {}, send() {} }
  } : { quasarRuntime: {} })
  await exposed.mini.tools.configure('UID', reactive({ serverIp: '192.168.127.1', poolStart: 100, poolEnd: 199, leaseSeconds: 3600 }))
  await exposed.mini.tools.forward('UID', reactive([{ id: 'r', port: 0, protocol: 'tcp', listenPort: 2222, targetIp: '192.168.127.100', targetPort: 22 }]))
  await exposed.mini.video.start('UID', reactive({ width: 1920, height: 1080, frameRate: 59.94 }), 1)
  await exposed.mini.tools.storage('UID', reactive({ kind: 'disk-open', id: 1, path: 1, writable: false }))
  await exposed.wake.wol(0, reactive({ enabled: true, mac: '02:03:04:05:06:07', localAddress: '192.168.0.1' }))
  assert.equal(ipcCalls.length, 5)
  console.log('PASS actual preload marshals reactive Vue configuration/modes/rules/actions into cloneable IPC data')
  const { MiniStorage, inspectOptical } = load('kbm/src-electron/minikmb-storage.ts')
  const { MiniNetworkWire } = load('kbm/src-electron/minikmb-network.ts')
  const { FolderDisk } = load('kbm/src-electron/folder-disk.ts')
  const { MiniCapture, MiniFrameParser } = load('kbm/src-electron/minikmb-capture.ts')
  const frame = (w = 1920, h = 1080) => { const b = Buffer.alloc(36); [0x4d4a5047, 4, w, h, 30000, 1001, 0, 0].forEach((n, i) => b.writeUInt32LE(n, i * 4)); b.set([255, 216, 255, 217], 32); return b }
  const parser = new MiniFrameParser(), packets = []
  for (const b of frame()) packets.push(...parser.feed(Uint8Array.of(b)))
  assert.equal(packets.length, 1); assert.equal(packets[0].frameRate, 30000 / 1001)
  assert.throws(() => new MiniFrameParser().feed(Buffer.alloc(32)))
  assert.throws(() => new MiniFrameParser().feed(Buffer.alloc(8 * 1024 * 1024 + 65537)))
  let live = 0, maxLive = 0, opens = [], sent = [], failed = []
  fakeSpawn = (_helper, args) => {
    const child = new EventEmitter(); child.stdout = new EventEmitter(); child.stderr = new EventEmitter(); child.stdin = new EventEmitter()
    child.stdin.end = () => { if (!child.closed) { child.closed = true; live--; setImmediate(() => child.emit('close', 0)) } }
    child.kill = child.stdin.end; live++; maxLive = Math.max(maxLive, live); opens.push(args)
    setTimeout(() => { if (child.closed) return; if (args[1] === '8192') { child.stderr.emit('data', Buffer.from('unsupported')); child.stdin.end() } else child.stdout.emit('data', frame(Number(args[1]) || 1920, Number(args[2]) || 1080)) }, 5)
    return child
  }
  const capture = new MiniCapture('fake.exe', f => sent.push(f), (uid, message) => failed.push({ uid, message }))
  const first = await capture.start('A', 'root-A', '1080', 1); assert.equal(first.width, 1920)
  await assert.rejects(capture.start('A', 'root-A', { width: 8192, height: 8192, frameRate: 30 }, 2))
  assert.equal(sent.at(-1).requestId, 2, 'failed exact tuple restores prior mode under new epoch')
  const a = capture.start('B', 'root-B', '720', 3).catch(() => {}), b = capture.start('C', 'root-C', '480', 4)
  await Promise.all([a, b]); assert.equal(maxLive, 1); assert.equal(opens.at(-1)[0], 'root-C')
  await capture.stop(); assert.equal(live, 0)
  console.log('PASS native frame bounds/splits, exact-root capture, one helper, rapid switch and fixed-mode rollback')

  let listener, requests = [], delivered = []
  const netClient = { subscribe: fn => { listener = fn; return () => {} }, command: async (ch, op, body) => { requests.push({ ch, op, body: Buffer.from(body) }); return new Uint8Array() } }
  const network = new MiniNetworkWire(netClient, f => delivered.push(f))
  await assert.rejects(network.send(Buffer.alloc(60)))
  await network.enable(true)
  assert.equal(requests[0].body.readBigUInt64LE(4), 32768n)
  const ethernet = Buffer.alloc(1514, 0xef); await network.send(ethernet)
  const fragments = requests.filter(r => r.op === 0x402)
  assert.equal(fragments.length, 2); assert.equal(fragments[1].body.readUInt16LE(4), 1008)
  assert.equal(fragments[0].body.readUInt32LE(8), crc32(ethernet))
  let sequence = 0
  for (const r of fragments) listener({ kind: 4, channel: 4, opcode: 0x480, sequence: ++sequence, payload: r.body })
  assert.equal(delivered.length, 1); assert.deepEqual(Buffer.from(delivered[0]), ethernet)
  const bad = Buffer.from(fragments[0].body); bad.writeUInt32LE(2, 0); bad[16] ^= 1
  listener({ kind: 4, channel: 4, opcode: 0x480, sequence: ++sequence, payload: bad })
  const badLast = Buffer.from(fragments[1].body); badLast.writeUInt32LE(2, 0)
  listener({ kind: 4, channel: 4, opcode: 0x480, sequence: ++sequence, payload: badLast })
  assert.equal(delivered.length, 1); assert.equal(network.dropped, 1)
  // A gap invalidates the partial frame; never deliver an unverified prefix.
  const gap = Buffer.from(fragments[1].body); gap.writeUInt32LE(3, 0)
  listener({ kind: 4, channel: 4, opcode: 0x480, sequence: sequence + 2, payload: gap })
  assert.equal(delivered.length, 1); network.dispose()
  console.log('PASS NCM firmware fragment ABI, cumulative credit, whole-frame CRC, gaps and disabled-link gating')

  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'minikmb-app-test-'))
  try {
    fs.writeFileSync(path.join(directory, 'test.txt'), 'immutable source')
    const folder = await FolderDisk.create(directory)
    const calls = [], jobs = new Map(), sectors = new Map(), catalog = new Map(); let nextJob = 0, disk = null, user = Buffer.alloc(0), staged = Buffer.alloc(0), expectedCrc = 0, optical = true
    const jobKinds = { 0x309: 5, 0x30d: 6, 0x30f: 14, 0x310: 7, 0x311: 8, 0x312: 9, 0x313: 10, 0x314: 11, 0x315: 12, 0x317: 13, 0x31b: 15, 0x31d: 16, 0x31f: 17 }
    let lastRead = 0, wrongKind = false
    const client = { command: async (ch, op, raw = new Uint8Array()) => {
      const body = Buffer.from(raw); calls.push({ ch, op, body })
      if (ch === 0 && op === 7) { const id = body.readUInt32LE(0), job = jobs.get(id), result = Buffer.alloc(32); result.writeUInt32LE(id, 0); result[4] = 1; result[5] = wrongKind ? 99 : job.kind; result.writeUInt32LE(job.object, 12); return result }
      if (ch === 0 && [8, 9].includes(op)) return Buffer.alloc(0)
      if (op === 0x305) throw Object.assign(Error('empty'), { status: 17 })
      if (op === 0x319) { const b = Buffer.alloc(8); b.writeUInt32LE(7); b[4] = b[5] = Number(optical); return b }
      if (op === 0x31a) { optical = Boolean(body[0]); return Buffer.alloc(0) }
      if (op === 0x31c) {
        const values = [...catalog.values()], b = Buffer.alloc(8 + values.length * 12); b[4] = values.length
        values.forEach((d, i) => { b.writeUInt32LE(d.id, 8 + i * 12); b.writeUInt32LE(d.sectors, 12 + i * 12); b[16 + i * 12] = d.format }); return b
      }
      if (op === 0x318) { if (!disk) throw Object.assign(Error('empty'), { status: 3 }); const r = Buffer.alloc(16); u32(disk.id, disk.sectors, disk.lease).copy(r); r[12] = disk.format; r[13] = disk.state; r[14] = disk.writable; return r }
      if (op === 0x308) { const r = Buffer.alloc(52); r[0] = 7; r.writeUInt16LE(user.length, 2); r.writeUInt32LE(3, 4); return r }
      if (op === 0x30b) { staged = Buffer.alloc(body.readUInt16LE(0)); return u32(71) }
      if (op === 0x30c) { assert.equal(body.readUInt32LE(0), 71); const at = body.readUInt16LE(4); body.subarray(6).copy(staged, at); const r = Buffer.alloc(2); r.writeUInt16LE(at + body.length - 6); return r }
      if (op === 0x30a) { assert.equal(body.readUInt32LE(0), 3); return user.subarray(body.readUInt16LE(4), body.readUInt16LE(4) + body.readUInt16LE(6)) }
      if (op === 0x316) return sectors.get(lastRead)?.subarray(body[8] * 256, (body[8] + 1) * 256) ?? Buffer.alloc(256)
      const kind = jobKinds[op]
      assert(kind, `unexpected opcode ${op.toString(16)}`)
      let object = 1
      if (op === 0x310) { disk = { id: body.readUInt32LE(0), sectors: body.readUInt32LE(4), format: body[8], lease: 33, state: 2, writable: 1 }; object = 33 }
      if (op === 0x315) { assert.equal(body.length, 520); sectors.set(body.readUInt32LE(4), Buffer.from(body.subarray(8))) }
      if (op === 0x314) lastRead = body.readUInt32LE(4)
      if (op === 0x311) { expectedCrc = body.readUInt32LE(4); catalog.set(disk.id, { ...disk }); disk = null }
      if (op === 0x31d) catalog.delete(body.readUInt32LE(0))
      if (op === 0x312) { disk = { id: body.readUInt32LE(0), sectors: folder.info.sectors, format: 1, lease: 44, state: [3, 4, 5][body[5]], writable: body[4] }; object = 44 }
      if (op === 0x313) disk = null
      if (op === 0x30d) user = Buffer.from(staged)
      const id = ++nextJob; jobs.set(id, { kind, object }); return u32(id)
    } }
    const storage = new MiniStorage(() => client, async kind => kind === 'folder' ? directory : undefined, async () => {})
    await storage.action({ kind: 'refresh' }); assert.equal(storage.state().image, null)
    const nameStart = calls.length
    await storage.action({ kind: 'channel-name-set', name: '서버 A' })
    assert.deepEqual(calls.slice(nameStart).map(c => c.op), [0x308, 0x31f, 7], 'name saves must not depend on optical/caps/image-catalog reads or list jobs')
    const rename = calls.find(c => c.op === 0x31f); assert(rename)
    assert.equal(rename.body.readUInt16LE(), Buffer.byteLength('서버 A')); assert.equal(rename.body.subarray(2).toString(), '서버 A')
    assert(!calls.some(c => c.op === 0x309), 'BOARD.ID initialization is autonomous firmware work')
    assert(!calls.some(c => [0x30b, 0x30c, 0x30d].includes(c.op)), 'name must not consume the 4KiB user value')
    await storage.action({ kind: 'optical-set', enabled: false }); assert.equal(storage.state().opticalApplied, false)
    await storage.action({ kind: 'optical-set', enabled: true }); assert.equal(storage.state().opticalApplied, true)
    await assert.rejects(storage.action({ kind: 'card-format', format: 2, confirmed: false }))
    assert(!calls.some(c => c.op === 0x30f))
    await assert.rejects(storage.action({ kind: 'card-user-write', text: 'x'.repeat(4097) }))
    await storage.action({ kind: 'card-user-write', text: '가'.repeat(1200) }); assert.equal(user.length, 3600)
    await storage.action({ kind: 'card-user-read' }); assert.equal(storage.state().userText, '가'.repeat(1200))
    await storage.action({ kind: 'folder-share' })
    assert.equal(storage.state().disk.state, 4); assert.equal(storage.state().disk.writable, false)
    assert.equal(storage.state().images.length, 1); assert.equal(storage.state().images[0].mounted, true)
    let checksum = 0
    for (let lba = 0; lba < folder.populatedSectors; lba += 8) {
      const block = Buffer.alloc(Math.min(8, folder.populatedSectors - lba) * 512)
      for (let n = 0; n < block.length / 512; n++) sectors.get(lba + n)?.copy(block, n * 512)
      checksum = crc32(block, checksum)
    }
    const zero = Buffer.alloc(1024 * 1024)
    for (let at = folder.populatedSectors * 512; at < folder.info.sectors * 512; at += zero.length) checksum = crc32(zero.subarray(0, Math.min(zero.length, folder.info.sectors * 512 - at)), checksum)
    assert.equal(expectedCrc, checksum); assert.equal(fs.readFileSync(path.join(directory, 'test.txt'), 'utf8'), 'immutable source')
    await assert.rejects(storage.action({ kind: 'disk-write', lba: 0, hex: '00'.repeat(512), confirmed: true }))
    await storage.action({ kind: 'disk-close' }); await storage.action({ kind: 'disk-open', id: 7, path: 0, writable: true })
    await storage.action({ kind: 'disk-read', lba: 0 }); assert.equal(storage.state().sectorHex.replace(/\s/g, '').length, 1024)
    await storage.action({ kind: 'disk-write', lba: 0, hex: 'ef'.repeat(512), confirmed: true }); assert.equal(sectors.get(0)[0], 0xef)
    wrongKind = true; await assert.rejects(storage.action({ kind: 'disk-sync' }), /불일치/); wrongKind = false
    console.log('PASS SD job identity/completion, 4KiB user profile, destructive confirmation, immutable folder snapshot CRC/zero tail, lease ownership and direct I/O')
  } finally { fs.rmSync(directory, { recursive: true, force: true }) }

  // Exercise real rename/readback orchestration without creating network state.
  cache.set(path.join(root, 'kbm/src-electron/network-service.ts'), { NetworkService: class {} })
  const { MiniFeatures } = load('kbm/src-electron/minikmb-features.ts')
  const logEvents = [], saved = { name: '', nameError: '' }, writes = []
  let mismatch = false, readFailure = false
  const features = new MiniFeatures({ refreshName: async () => { saved.nameError = readFailure ? 'SD 읽기 실패' : ''; if (mismatch) saved.name = '' }, channel: () => saved }, (...event) => logEvents.push(event), async () => undefined)
  features.context = async () => ({ storage: { action: async action => { writes.push(action); saved.name = action.name } } })
  await features.rename('UID', ' 서버 A '); assert.equal(saved.name, '서버 A')
  assert.deepEqual(writes.at(-1), { kind: 'channel-name-set', name: '서버 A' })
  mismatch = true; await assert.rejects(features.rename('UID', '서버 A'), /이름이 다릅니다/); mismatch = false
  readFailure = true; await assert.rejects(features.rename('UID', '서버 A'), /SD 읽기 실패/)
  assert(logEvents.every(event => Object.keys(event[2]).join() === 'uid'), 'logs never contain names')
  console.log('PASS name-only storage dependencies, normalized save/readback, mismatch/read errors and private lifecycle logs')

  const sectors = new Map(), block = lba => { if (!sectors.has(lba)) sectors.set(lba, Buffer.alloc(2048)); return sectors.get(lba) }
  const pvd = block(16); pvd[0] = 1; pvd.write('CD001', 1); pvd[6] = 1; pvd.writeUInt32LE(40, 80); pvd.writeUInt32BE(40, 84); pvd.writeUInt16LE(2048, 128); pvd.writeUInt16BE(2048, 130)
  const boot = block(17); boot.write('CD001', 1); boot[6] = 1; boot.write('EL TORITO SPECIFICATION', 7); boot.writeUInt32LE(20, 71)
  const end = block(18); end[0] = 255; end.write('CD001', 1); end[6] = 1
  const catalog = block(20); catalog[0] = 1; catalog[30] = 0x55; catalog[31] = 0xaa; catalog.writeUInt16LE(0x55aa, 28); catalog[32] = 0x88; catalog.writeUInt16LE(1, 38); catalog.writeUInt32LE(21, 40)
  await inspectOptical(40 * 2048, async lba => block(lba))
  catalog[32] = 0; await assert.rejects(inspectOptical(40 * 2048, async lba => block(lba)), /부팅 항목/)
  console.log('PASS limited common ISO/El Torito profile rejects non-bootable images; no physical writes or HID')
}
main().catch(e => { console.error(e); process.exitCode = 1 })
