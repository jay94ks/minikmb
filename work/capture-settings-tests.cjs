// Windows-only app tests: mocked browser media, storage, and Vue renderer.
// No serial, HID, camera, or actual USB access.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict')
const base = path.resolve(__dirname, '../kbm'), ts = require('../kbm/node_modules/typescript')
const vue = require('../kbm/node_modules/vue'), sfc = require('../kbm/node_modules/@vue/compiler-sfc')
const cache = new Map(), store = new Map(), calls = [], streams = [], events = []
global.localStorage = { getItem: key => store.get(key) ?? null, setItem: (key, value) => store.set(key, value) }
let physicalOrder = ['board-a', 'board-b'], rejectMode = () => false, mismatched = false
global.window = { kbmNative: { video: { listPnp: async () => [
  { name: 'USB3. 0 capture', instanceId: 'USB\\VID_534D&PID_2109\\LOCATION-B', vendorId: '534d', productId: '2109' },
  { name: 'USB3. 0 capture', instanceId: 'USB\\VID_534D&PID_2109\\LOCATION-A', vendorId: '534d', productId: '2109' }
] } } }
function makeStream(settings) {
  const track = { readyState: 'live', stopped: false, addEventListener() {}, removeEventListener() {},
    stop() { this.stopped = true; this.readyState = 'ended'; events.push('stop') }, getSettings: () => settings }
  const stream = { getVideoTracks: () => [track], getTracks: () => [track], track }
  streams.push(stream); return stream
}
Object.defineProperty(global, 'navigator', { configurable: true, value: { mediaDevices: {
  enumerateDevices: async () => physicalOrder.map(deviceId => ({ deviceId, groupId: deviceId,
    label: 'USB3. 0 capture (534d:2109)', kind: 'videoinput' })),
  getUserMedia: async constraints => {
    calls.push(constraints); events.push('open')
    if (rejectMode(constraints)) throw new DOMException('Unsupported tuple', 'OverconstrainedError')
    const c = constraints.video
    return makeStream({ width: mismatched ? 320 : (c.width?.exact ?? 1920),
      height: c.height?.exact ?? 1080, frameRate: c.frameRate?.exact ?? 30 })
  }
} } })
global.document = { createElement: () => ({ videoWidth: 1920, videoHeight: 1080, muted: true,
  requestVideoFrameCallback(callback) { queueMicrotask(callback); return 1 }, cancelVideoFrameCallback() {},
  play: async () => {}, pause() {}, srcObject: null }) }
function load(file) {
  if (cache.has(file)) return cache.get(file)
  let source = fs.readFileSync(file, 'utf8')
  if (file.endsWith('.vue')) {
    const { descriptor, errors } = sfc.parse(source, { filename: file }); assert.deepEqual(errors, [])
    source = sfc.compileScript(descriptor, { id: file, inlineTemplate: true }).content
  }
  const exports = {}; cache.set(file, exports)
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS,
    target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
  vm.runInThisContext(`(function(exports,require){${js}\n})`, { filename: file })(exports, name => {
    if (name === 'vue') return vue
    if (name.startsWith('.')) return load(path.resolve(path.dirname(file), name + (path.extname(name) ? '' : '.ts')))
    throw Error(`Unexpected dependency: ${name}`)
  }); return exports
}
const prefsFile = path.join(base, 'src/services/capture-preferences.ts')
const prefs = load(prefsFile), media = load(path.join(base, 'src/services/capture-devices.ts'))
const fixed = (width = 1280, height = 720, frameRate = 24) => ({ kind: 'fixed', width, height, frameRate })
const blacklist = { names: [], usbIds: [] }
const node = (type, text = '') => ({ type, text, props: {}, children: [], parent: null })
const renderer = vue.createRenderer({
  createElement: tag => node(tag), createText: text => node('#text', text), createComment: text => node('#comment', text),
  setText: (n, text) => n.text = text, setElementText: (n, text) => { n.text = text; n.children = [] },
  patchProp: (n, key, old, value) => n.props[key] = value, parentNode: n => n.parent,
  nextSibling: n => { const a = n.parent?.children ?? []; return a[a.indexOf(n) + 1] ?? null },
  insert(n, parent, anchor) { if (n.parent) { const a = n.parent.children; a.splice(a.indexOf(n), 1) }
    n.parent = parent; const i = anchor ? parent.children.indexOf(anchor) : -1; parent.children.splice(i < 0 ? parent.children.length : i, 0, n) },
  remove(n) { if (n.parent) { const a = n.parent.children; a.splice(a.indexOf(n), 1); n.parent = null } }
})
const walk = n => [n, ...n.children.flatMap(walk)]
const text = n => n.text + n.children.map(text).join('')
const settle = async () => { for (let i = 0; i < 6; i++) { await Promise.resolve(); await vue.nextTick() } }
;(async () => {
  assert.equal(prefs.validCaptureMode(fixed(1920, 1080, 59.94)), true)
  for (const mode of [fixed(0), fixed(1280.5), fixed(9000), fixed(1280, 720, NaN), fixed(1280, 720, 0), fixed(1280, 720, 241)]) {
    assert.equal(prefs.validCaptureMode(mode), false)
  }
  assert.equal(prefs.modeLabel(fixed()), '1280 x 720 / 24 Hz')
  prefs.saveCaptureAssignments(['board-b', 'board-a'])
  let devicePorts = prefs.initializeCaptureUartPorts(['board-b', 'board-a'], [1, 0])
  assert.deepEqual(devicePorts, { 'board-b': 1, 'board-a': 0 }, 'Migrate old screen-based ports once')
  devicePorts = prefs.initializeCaptureUartPorts(['board-a', 'board-b'], [1, 0])
  assert.deepEqual(prefs.captureScreenPorts(['board-a', 'board-b'], devicePorts, [1, 0]), [0, 1], 'UART follows the board on swap')
  devicePorts = prefs.saveCaptureUartPort('board-a', 1)
  assert.deepEqual(prefs.captureScreenPorts(['board-a', 'board-b'], devicePorts, [0, 0]), [1, 1], 'Both boards may route to the same UART')
  devicePorts = prefs.saveCaptureUartPort('board-a', 0)
  assert.deepEqual(prefs.captureScreenPorts([null, 'board-b'], devicePorts, [0, 0]), [0, 1], 'Unassigned slots retain legacy fallback')
  assert.deepEqual(prefs.captureScreenPorts(['board-b', 'board-a'], devicePorts, [0, 1]), [1, 0], 'A missing board keeps its saved UART')
  assert.throws(() => prefs.saveCaptureUartPort('board-a', 2))
  assert.throws(() => prefs.saveCaptureUartPort('', 0))
  prefs.saveCaptureMode('board-a', fixed())
  prefs.saveCaptureMode('board-b', fixed(1920, 1080, 59.94))
  assert.deepEqual(prefs.loadCaptureMode('board-a'), fixed())
  assert.deepEqual(prefs.loadCaptureAssignments(), ['board-b', 'board-a'])
  cache.delete(prefsFile)
  assert.deepEqual(load(prefsFile).loadCaptureMode('board-a'), fixed()) // Reload/startup.
  assert.deepEqual(load(prefsFile).loadCaptureUartPorts(), { 'board-b': 1, 'board-a': 0 })
  assert.deepEqual(load(prefsFile).captureScreenPorts(['board-a', 'board-b'],
    load(prefsFile).loadCaptureUartPorts(), [1, 0]), [0, 1], 'Startup restores board-based routing, not slot defaults')
  assert.throws(() => prefs.saveCaptureAssignments(['board-a', 'board-a']))
  assert.deepEqual(prefs.resolveCaptureAssignments(['board-b', 'board-a'], ['board-a']), ['board-b', 'board-a'])
  assert.deepEqual(prefs.resolveCaptureAssignments([null, null], ['board-a', 'board-b']), ['board-a', 'board-b'])
  const opened = await media.openCaptureDevices(blacklist)
  assert.deepEqual(opened.assignments, ['board-b', 'board-a'])
  assert.equal(opened.slots[0].deviceId, 'board-b'); assert.equal(opened.slots[1].deviceId, 'board-a')
  assert.equal(opened.slots[0].frameRate, 59.94)
  assert(opened.inventory.every(d => !d.pnp), 'Ambiguous duplicate names must not invent PnP associations')
  assert(calls.every(c => c.audio === false && c.video.resizeMode.exact === 'none'))
  const otherStream = opened.slots[0].stream, previous = opened.slots[1], device = opened.inventory.find(d => d.deviceId === 'board-a')
  events.length = 0
  const changed = await media.replaceCaptureMode(device, previous, fixed(640, 480, 30))
  assert.equal(changed.applied, true); assert.equal(previous.stream.track.stopped, true)
  assert.equal(events[0], 'stop'); assert.equal(events[1], 'open')
  assert.equal(otherStream.track.stopped, false, 'Changing A must not stop B')
  rejectMode = c => c.video.width?.exact === 8192
  const before = calls.length
  const failed = await media.replaceCaptureMode(device, changed.slot, fixed(8192, 8192, 30))
  assert.equal(failed.applied, false); assert.equal(failed.slot.width, 640)
  assert.equal(calls.length - before, 2, 'Only requested tuple and prior tuple: no silent fallback')
  assert.deepEqual(prefs.loadCaptureMode('board-a'), fixed(), 'Rejected mode must not overwrite saved preferences')
  assert.equal(otherStream.track.stopped, false)
  rejectMode = () => false; mismatched = true
  const badSettings = await media.replaceCaptureMode(device, undefined, fixed())
  assert.equal(badSettings.applied, false); assert.equal(streams.at(-1).track.stopped, true)
  mismatched = false
  physicalOrder = ['board-a']; const missing = await media.openCaptureDevices(blacklist)
  assert.equal(missing.slots[0], undefined); assert.equal(missing.slots[1].deviceId, 'board-a')
  physicalOrder = ['board-b', 'board-a']; const reordered = await media.openCaptureDevices(blacklist)
  assert.deepEqual(reordered.slots.map(s => s.deviceId), ['board-b', 'board-a'])
  const autoCalls = calls.length
  rejectMode = c => c.video.width?.exact === 1920
  const automatic = await media.openCaptureDevice(device, { kind: 'auto' })
  assert.deepEqual(calls.slice(autoCalls).map(c => [c.video.width?.exact, c.video.frameRate?.exact]),
    [[1920, 30], [1920, 24], [1920, undefined], [1280, 30]])
  assert.equal(automatic.width, 1280); rejectMode = () => false

  // Real Vue dialog event handlers with Quasar controls stubbed, no IPC/HID.
  const component = load(path.join(base, 'src/components/CaptureSettingsDialog.vue')).default
  const emitted = [], props = vue.reactive({ inventory: opened.inventory, assignments: ['board-b', 'board-a'],
    captures: opened.slots, uartPorts: [1, 0], busy: false, onMode: (...args) => emitted.push(['mode', ...args]),
    onAssign: a => emitted.push(['assign', a]), onPrepare: () => emitted.push(['prepare']) })
  const container = node('root'), app = renderer.createApp({ render: () => vue.h(component, props) })
  for (const name of ['btn', 'tooltip', 'dialog', 'card', 'card-section', 'space', 'select', 'input', 'separator']) {
    app.component(`q-${name}`, vue.defineComponent({ inheritAttrs: false, setup(_, ctx) { return () =>
      name === 'dialog' && !ctx.attrs.modelValue ? null : vue.h(name === 'btn' ? 'button' : 'div',
        { ...ctx.attrs, 'data-q': name }, [ctx.attrs.label ?? '', ...(ctx.slots.default?.() ?? [])]) } }))
  }
  app.directive('close-popup', { mounted() {} }); app.mount(container)
  const control = label => { const n = walk(container).find(n => n.props.label === label || n.props['aria-label'] === label); assert(n, label); return n }
  const click = async label => { await control(label).props.onClick(); await settle() }
  try {
    await settle(); assert.deepEqual(emitted, [])
    await click('캡처 해상도 및 장치 순서 설정'); assert.deepEqual(emitted, [['prepare']]); emitted.length = 0
    assert(text(container).includes('제어 대상: UART 1'))
    assert(text(container).includes('제어 대상: UART 0'))
    const modeSelect = control('화면 1 영상 모드 (W x H / Hz)')
    assert(modeSelect.props.options.some(o => o.label === '1920 x 1080 / 30 Hz'))
    assert(modeSelect.props.options.some(o => o.label === '수동 지정'))
    modeSelect.props['onUpdate:modelValue']('manual'); await settle()
    const inputs = walk(container).filter(n => n.props['data-q'] === 'input')
    for (const [i, value] of [1600, 900, 59.94].entries()) inputs[i].props['onUpdate:modelValue'](value)
    await settle(); await click('화면 1 모드 저장 및 적용')
    assert.deepEqual(emitted.pop(), ['mode', 0, fixed(1600, 900, 59.94)])
    inputs[0].props['onUpdate:modelValue'](0); await settle()
    assert.equal(control('화면 1 모드 저장 및 적용').props.disable, true)
    await click('화면 1 ↔ 2 바꾸기'); await click('장치 순서 저장 및 적용')
    assert.deepEqual(emitted.pop(), ['assign', ['board-a', 'board-b']])
    props.busy = true; await settle(); assert.equal(control('장치 순서 저장 및 적용').props.disable, true)
    assert(text(container).includes('고유 시리얼을 보장하지 않습니다'))
  } finally { app.unmount(); media.stopCaptureDevices(streams.map(stream => ({ stream }))) }
  const page = fs.readFileSync(path.join(base, 'src/pages/IndexPage.vue'), 'utf8')
  assert(page.includes('captureDisposed')); assert(page.includes('captureRefreshPending'))
  assert(page.includes('srcObject !==')); assert(!fs.readFileSync(path.join(base, 'src/services/capture-devices.ts'), 'utf8').includes('.applyConstraints('))
  assert(page.includes('const screenPorts = computed(() => captureScreenPorts('))
  assert(page.indexOf('releaseInputs()', page.indexOf('async function assignCaptureDevices')) <
    page.indexOf('captureAssignments.value = [...assignments]', page.indexOf('async function assignCaptureDevices')))
  assert(page.includes('saveCaptureUartPort(deviceId, value)'))
  console.log('PASS capture settings: persisted board UART mapping + legacy migration/swap/reload/missing slots, device assignments/modes, exact constraints, first frame, targeted stop/reopen/rollback, auto fallback, validation, actual Vue events; no USB/serial/HID access')
})().catch(error => { console.error(error); process.exitCode = 1 })
