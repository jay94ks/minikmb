// Real main-process keyboard bridge with a fake native pipe; no hook/HID input.
const fs = require('node:fs'), vm = require('node:vm'), path = require('node:path'), assert = require('node:assert/strict'), { EventEmitter } = require('node:events')
const ts = require('../kbm/node_modules/typescript'), api = {}, children = [], UID = '240043001451333532363439', OTHER = '123456789012345678901234'
let ackFixture = true
function spawn() {
  const child = new EventEmitter(); child.stdout = new EventEmitter(); child.stderr = { resume() {} }; child.stdin = new EventEmitter(); child.commands = []
  child.stdin.writable = true
  child.stdin.write = line => { child.commands.push(line); const [mode, generation] = line.trim().split(' '); const extra = mode === 'E' && ackFixture ? `K ${generation} 6 1\nK ${generation} 6 0\n` : ''; if (mode === 'E') ackFixture = false; queueMicrotask(() => child.stdout.emit('data', Buffer.from(`S ${generation} ${mode === 'E' ? 1 : 0}\n${extra}`))) }
  child.stdin.end = () => { child.stdin.writable = false; queueMicrotask(() => child.emit('close', 0)) }; child.kill = child.stdin.end
  children.push(child); setImmediate(() => child.stdout.emit('data', Buffer.from('READY\n'))); return child
}
const file = path.resolve(__dirname, '../kbm/src-electron/minikmb-keyboard.ts')
const js = ts.transpileModule(fs.readFileSync(file, 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText
vm.runInThisContext(`(function(exports,require){${js}\n})`, { filename: file })(api, n => n === 'node:child_process' ? { spawn } : require(n))
const tick = () => new Promise(r => setImmediate(r))
;(async () => {
  let selected = UID, focused = true, failDown = false, finish, finishPrepare, holdPrepare = false
  const sent = [], released = [], states = []
  const bridge = new api.MiniKeyboard('fake-helper', uid => uid === selected && focused, async (uid, input, current) => { if (holdPrepare) await new Promise(resolve => { finishPrepare = resolve }); if (!current()) return; sent.push({ uid, ...input }); if (failDown) throw Error('lost ACK'); if (finish === null) await new Promise(resolve => { finish = resolve }) }, async uid => { released.push(uid) }, state => states.push(state))
  let result = await bridge.set(UID, true, '123'); assert(result.active)
  await tick(); assert.deepEqual(sent.map(x => [x.usage, x.down]), [[6, true], [6, false]], 'keys sharing a native chunk with enable ACK must not be dropped'); sent.length = 0
  let child = children.at(-1), generation = Number(child.commands.at(-1).trim().split(' ')[1])
  child.stdout.emit('data', Buffer.from(`K ${generation} 227 1\nK ${generation} 21 1\nK ${generation} 21 0\nK ${generation} 227 0\n`)); await tick()
  assert.deepEqual(sent.map(x => [x.usage, x.down]), [[227, true], [21, true], [21, false], [227, false]])
  assert(sent.every(x => x.uid === UID))
  await bridge.deactivate(); const before = sent.length
  const releaseCount = released.length; await bridge.deactivate(); assert.equal(released.length, releaseCount, 'inactive focus checks must not release unrelated fresh DOM input')
  child.stdout.emit('data', Buffer.from(`K ${generation} 4 1\n`)); await tick(); assert.equal(sent.length, before)
  await bridge.set(UID, true, '123'); focused = false; bridge.check(); await tick(); assert(!states.at(-1).active)
  focused = true; selected = OTHER; await bridge.set(OTHER, true, '123'); generation = bridge.generation
  child.stdout.emit('data', Buffer.from(`K ${generation - 1} 4 1\nK ${generation} 144 1\nK ${generation} 144 0\n`)); await tick()
  assert(sent.slice(before).every(x => x.uid === OTHER)); assert(released.includes(UID))
  child.stdout.emit('data', Buffer.from(`R ${generation} 0\n`)); await tick(); assert(!states.at(-1).active)
  await bridge.set(OTHER, true, '123'); holdPrepare = true; generation = bridge.generation
  const beforePrepare = sent.length
  child.stdout.emit('data', Buffer.from(`K ${generation} 7 1\n`)); await tick(); await bridge.deactivate(); finishPrepare(); holdPrepare = false; await tick()
  assert.equal(sent.length, beforePrepare, 'a DOWN awaiting preparation must be canceled before native release')
  await bridge.set(OTHER, true, '123'); failDown = true; generation = bridge.generation
  child.stdout.emit('data', Buffer.from(`K ${generation} 4 1\n`)); await tick(); await tick()
  assert.equal(sent.filter(x => x.usage === 4 && x.down).length, 1, 'lost DOWN must not retry')
  assert(states.some(x => x.error)); failDown = false
  await bridge.set(OTHER, true, '123'); child = children.at(-1); generation = bridge.generation; finish = null
  child.stdout.emit('data', Buffer.from(Array.from({ length: 70 }, () => `K ${generation} 5 1\n`).join(''))); await tick()
  assert(!states.at(-1).active, 'overflow stops capture and releases original UID')
  finish?.(); await tick(); await bridge.stop()
  console.log('PASS native bridge UID isolation, ordered DOWN/UP, stale generation discard, foreground release, reset, no DOWN retry, bounded queue and shutdown; no physical key capture')
})().catch(e => { console.error(e); process.exitCode = 1 })
