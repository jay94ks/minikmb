// Actual opt-in Electron diagnostic module, mocked Electron; no device access.
const fs = require('node:fs'), path = require('node:path'), os = require('node:os')
const vm = require('node:vm'), assert = require('node:assert/strict')
const ts = require('../kbm/node_modules/typescript')
const source = fs.readFileSync(path.join(__dirname, '../kbm/src-electron/ui-benchmark.ts'), 'utf8')
const handlers = new Map(), paths = new Map(), timers = [], events = new Map()
let permissions, requested, options, loaded = false
const session = { setPermissionCheckHandler: f => { permissions = f }, setPermissionRequestHandler: f => { requested = f } }
class Window {
  constructor(value) { options = value; this.webContents = {
    on: (name, fn) => events.set(name, fn),
    setBackgroundThrottling: value => assert.equal(value, true),
    executeJavaScript: async () => { await handlers.get('minikmb:scan')() },
    capturePage: async () => ({ isEmpty: () => false, toPNG: () => Buffer.from('fixture'), getSize: () => ({ width: 1600, height: 1025 }) })
  } }
  setIgnoreMouseEvents(value) { assert.equal(value, true) }
  setContentSize(width, height) { assert.deepEqual([width, height], [1280, 820]) }
  showInactive() {}
  on() {}
  once(_name, callback) { callback() }
  getBounds() { return {} }
  getContentSize() { return [1280, 820] }
}
const electron = { app: { setPath: (key, value) => paths.set(key, value), quit() {} },
  ipcMain: { handle: (key, fn) => handlers.set(key, fn) }, Menu: { setApplicationMenu() {} },
  session: { defaultSession: session }, BrowserWindow: Window, screen: { getDisplayMatching: () => ({ scaleFactor: 1.25 }) } }
const moduleValue = { exports: {} }
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
vm.runInNewContext(compiled, { exports: moduleValue.exports, require: name => name === 'electron' ? electron : require(name),
  process, Buffer, setTimeout: fn => { timers.push(fn); return 1 }, setInterval: fn => { timers.push(fn); return 1 }, clearTimeout() {}, clearInterval() {} })
const { prepareUiBenchmark, runUiBenchmark } = moduleValue.exports
const parent = fs.realpathSync(os.tmpdir()), root = fs.mkdtempSync(path.join(parent, 'minikmb-ui-benchmark-'))
;(async () => {
  assert.equal(prepareUiBenchmark([]), undefined)
  assert.throws(() => prepareUiBenchmark(['--ui-benchmark', 'relative']))
  assert.throws(() => prepareUiBenchmark(['--ui-benchmark', root, '--mini-inspect']))
  assert.throws(() => prepareUiBenchmark(['--ui-benchmark', root]))
  const directory = path.join(root, 'new')
  assert.equal(prepareUiBenchmark(['--ui-benchmark', directory]), directory)
  assert.equal(paths.get('userData'), path.join(directory, 'profile'))
  assert.equal(paths.get('sessionData'), paths.get('userData'))
  await runUiBenchmark(directory, 'fixture-preload', async () => { loaded = true; await handlers.get('minikmb:scan')() })
  assert(loaded)
  assert.equal(options.webPreferences.contextIsolation, true)
  assert.equal(options.webPreferences.nodeIntegration, false)
  assert.equal(options.webPreferences.sandbox, true)
  assert.equal(options.focusable, false)
  assert.equal(permissions(), false)
  requested(null, 'media', value => assert.equal(value, false))
  assert.deepEqual([...handlers.keys()].sort(), ['minikmb:scan', 'minikmb:state', 'minikmb:keyboard-capture', 'minikmb:release-input', 'minikmb:video-stop'].sort())
  assert.equal(handlers.has('minikmb:input'), false)
  let prevented = false
  events.get('before-input-event')({ preventDefault() { prevented = true } })
  assert(prevented)
  const ready = JSON.parse(fs.readFileSync(path.join(directory, 'ready.json')))
  assert.equal(ready.devices, 'disabled'); assert.deepEqual(ready.content_dip, [1280, 820])
  assert.equal(timers.length, 2)
  console.log('UI benchmark: exclusive profile, no device IPC, permissions denied, sandbox and real page load passed')
})().catch(error => { console.error(error); process.exitCode = 1 }).finally(() => {
  assert.equal(path.dirname(path.resolve(root)), parent)
  assert(path.basename(root).startsWith('minikmb-ui-benchmark-'))
  fs.rmSync(root, { recursive: true })
})
