// Actual storage Vue component; all IPC and dialogs are mocked. No device writes.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict')
const ts = require('../kbm/node_modules/typescript'), vue = require('../kbm/node_modules/vue'), sfc = require('../kbm/node_modules/@vue/compiler-sfc')
const { h, defineComponent, createRenderer, nextTick } = vue, cache = new Map(), calls = [], dialogs = [], UID = '240043001451333532363439'
const images = [
  { id: 1, kind: 'optical', name: 'ipxe.iso', size: 9437184, format: 0, mounted: false, path: 1, writable: false },
  { id: 37, kind: 'disk', name: 'D0000025.IMG', size: 67108864, format: 3, mounted: false, path: 0, writable: true }
]
const state = { features: 0x77, input: { busy: false }, serial: { received: [], bytesReceived: 0 }, network: { enabled: [false], rxFrames: [0], txFrames: [0] }, storage: { capabilities: 7, images, opticalApplied: true, busy: false, image: images[0], disk: null } }
const api = { state: async () => structuredClone(state), storage: async (uid, action) => { calls.push({ uid, action }); if (action.kind === 'optical-set') state.storage.opticalApplied = action.enabled }, network: async (uid, mode) => { calls.push({ uid, action: { kind: 'network', mode } }); state.networkMode = mode; state.network.enabled[0] = mode !== 'off' }, scan: async (uid, request) => { calls.push({ uid, action: { kind: 'scan', request } }); state.scan = { running: Boolean(request), completed: 0, total: request ? request.end - request.start + 1 : 0, open: [], closed: 0, filtered: 0, error: '' } }, configure: async () => {}, deactivate: async () => {} }
global.window = { kbmNative: { mini: { tools: api }, wake: { adapters: async () => [] } }, addEventListener() {}, removeEventListener() {} }
global.localStorage = { getItem: () => null, setItem() {} }
const stub = name => defineComponent({ inheritAttrs: false, setup(_, ctx) { return () => h(name === 'btn' ? 'button' : 'div', { ...ctx.attrs, 'data-q': name }, [ctx.attrs.label ?? '', ...(ctx.slots.default?.() ?? [])]) } })
function load(file) {
  if (file.endsWith('OperationLogs.vue')) return { default: stub('logs'), __esModule: true }
  if (cache.has(file)) return cache.get(file)
  let source = fs.readFileSync(file, 'utf8')
  if (file.endsWith('.vue')) { const { descriptor, errors } = sfc.parse(source, { filename: file }); assert.deepEqual(errors, []); source = sfc.compileScript(descriptor, { id: file, inlineTemplate: true }).content }
  const exports = {}; cache.set(file, exports)
  const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
  vm.runInThisContext(`(function(exports,require){${js}\n})`, { filename: file })(exports, name => {
    if (name === 'vue') return vue
    if (name === 'quasar') return { QDialog: stub('dialog'), useQuasar: () => ({ dialog: options => { dialogs.push(options); const chain = { onOk(fn) { queueMicrotask(() => fn(options.prompt ? '128' : undefined)); return chain }, onCancel() { return chain }, onDismiss() { return chain } }; return chain } }) }
    if (name.startsWith('.')) return load(path.resolve(path.dirname(file), name + (path.extname(name) ? '' : '.ts')))
    throw Error(name)
  }); return exports
}
const node = (type, text = '') => ({ type, text, props: {}, children: [], parent: null })
const renderer = createRenderer({
  createElement: t => node(t), createText: t => node('#text', t), createComment: t => node('#comment', t),
  setText: (n, t) => { n.text = t }, setElementText: (n, t) => { n.text = t; n.children = [] }, patchProp: (n, k, _, v) => { n.props[k] = v }, parentNode: n => n.parent,
  nextSibling: n => n.parent?.children[n.parent.children.indexOf(n) + 1] ?? null,
  insert(n, parent, anchor) { if (n.parent) n.parent.children.splice(n.parent.children.indexOf(n), 1); n.parent = parent; const i = anchor ? parent.children.indexOf(anchor) : -1; parent.children.splice(i < 0 ? parent.children.length : i, 0, n) },
  remove(n) { if (n.parent) n.parent.children.splice(n.parent.children.indexOf(n), 1); n.parent = null }
})
const walk = n => [n, ...n.children.flatMap(walk)], text = n => n.text + n.children.map(text).join('')
const settle = async () => { for (let i = 0; i < 30; i++) { await Promise.resolve(); await nextTick() } }
async function main() {
  const app = renderer.createApp(load(path.resolve(__dirname, '../kbm/src/components/MiniChannelTools.vue')).default, { uid: UID, console: true, beforeInput: async () => {} }), container = node('root')
  for (const n of ['card', 'space', 'btn', 'tabs', 'tab', 'banner', 'tab-panels', 'tab-panel', 'chip', 'select', 'expansion-item', 'toggle', 'input', 'linear-progress', 'list', 'item', 'item-section', 'item-label', 'icon', 'menu', 'separator']) app.component(`q-${n}`, stub(n))
  app.directive('close-popup', {})
  const button = label => { const n = walk(container).find(n => n.type === 'button' && n.props.label === label); assert(n, label); return n }
  const click = async n => { assert(!n.props.disable, 'button enabled'); await n.props.onClick?.(); await settle() }
  const instance = app.mount(container)
  try {
    await settle(); instance.show('storage'); await settle()
    const left = walk(container).find(n => n.props.class === 'storage-functions'), right = walk(container).find(n => n.props.class === 'storage-images')
    assert(left && right); assert(walk(right).some(n => n.props.icon === 'add')); assert(!walk(left).some(n => n.props.icon === 'add'))
    assert(!walk(left).some(n => n.props.label === '포맷하기')); assert(walk(left).some(n => n.props.label === '선택 광학 연결'))
    assert(text(right).includes('ipxe.iso') && text(right).includes('D0000025.IMG') && text(right).includes('EXT4'))
    const disk = walk(right).find(n => n.props['data-q'] === 'item' && text(n).includes('D0000025.IMG')); await click(disk)
    assert(!walk(left).some(n => n.props.label === '선택 광학 연결')); assert(walk(left).some(n => n.props.label === '포맷하기'))
    const format = walk(left).find(n => n.props.label === '이미지 파일시스템'); format.props['onUpdate:modelValue'](2); await settle()
    await click(button('포맷하기'))
    assert.deepEqual(calls.at(-1), { uid: UID, action: { kind: 'disk-format', id: 37, format: 2, confirmed: true } }); assert(dialogs.at(-1).message.includes('D0000025.IMG'))
    const create = walk(right).find(n => n.props['data-q'] === 'item' && text(n) === 'exFAT 이미지 만들기'); await click(create)
    assert.deepEqual(calls.at(-1), { uid: UID, action: { kind: 'disk-create', format: 2, sizeMiB: 128 } })
    state.storage.disk = { id: 88, sectors: 131072, lease: 0, state: 0, format: 1, writable: false }
    await click(button('목록 갱신'))
    await click(button('선택 이미지 열기')); assert.equal(calls.at(-1).action.id, 37, 'refresh must not replace the selected ID with an unrelated last-used disk')
    assert(button('동기화').props.disable, 'unrelated active disk must not accept selected-image I/O')
    const optical = walk(right).find(n => n.props['data-q'] === 'item' && text(n).includes('ipxe.iso')); await click(optical); assert(!walk(left).some(n => n.props.label === '포맷하기'))
    const toggle = walk(left).find(n => n.props.label === '광학 드라이브 에뮬레이션'); await toggle.props['onUpdate:modelValue'](false); await settle()
    assert.deepEqual(calls.at(-1), { uid: UID, action: { kind: 'optical-set', enabled: false } })
    state.storage.busy = true; state.storage.transfer = { bytesPerSecond: 262144, remainingSeconds: 125, elapsedSeconds: 5 }; state.storage.done = 1048576; state.storage.total = 33554432
    const refresh = walk(container).find(n => n.props.icon === 'refresh' && !n.props.label); await click(refresh)
    assert(text(container).includes('256.0 KiB/s')); assert(text(container).includes('2분 5초'))
    state.storage.transfer = null; state.storage.busy = false; await click(refresh); assert(!text(container).includes('KiB/s'))
    instance.show('network'); await settle(); assert(button('스캔 시작').props.disable)
    await click(button('호스트 네트워크 공유 켜기')); assert.equal(calls.at(-1).action.mode, 'host'); assert(!button('스캔 시작').props.disable)
    await click(button('스캔 시작')); assert.equal(calls.at(-1).action.request.target, '192.168.127.100')
    await click(button('스캔 중지')); assert.equal(calls.at(-1).action.request, null)
    await click(button('에뮬레이션 모드로 켜기')); assert.equal(calls.at(-1).action.mode, 'emulation')
    assert(calls.every(c => c.uid === UID)); assert(!calls.some(c => c.action.kind === 'card-format'))
    console.log('PASS actual Vue two-pane SD catalog, selected-image format/confirmation, create format/size, selected ID open, optical OFF and UID isolation; mocked IPC only')
  } finally { app.unmount(); await settle() }
}
main().catch(e => { console.error(e); process.exitCode = 1 })
