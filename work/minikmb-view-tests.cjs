// Actual Vue page event handlers with an in-memory renderer and mocked IPC only.
const fs = require('node:fs'), path = require('node:path'), vm = require('node:vm'), assert = require('node:assert/strict')
const ts = require('../kbm/node_modules/typescript'), vue = require('../kbm/node_modules/vue'), sfc = require('../kbm/node_modules/@vue/compiler-sfc')
const { h, reactive, defineComponent, createRenderer, nextTick } = vue, root = path.resolve(__dirname, '..'), cache = new Map(), calls = [], events = new Map()
const UID = '240043001451333532363439', OTHER = '123456789012345678901234'
const channel = (uid, slot) => ({ uid, connected: true, features: 0x77, targetState: 1, slot, usbPort: slot + 3, captureId: `root-${slot}`, pairingError: '', boardId: 'board', port: 'unused', error: '' })
const inventory = { boards: [{ id: 'board', label: 'MiniKMB', channels: [channel(UID, 0), channel(OTHER, 1)], captures: [], pairing: '' }], error: '', scannedAt: 1 }
const route = reactive({ name: undefined, fullPath: '/', params: {} }), router = { push: async value => { if (typeof value === 'string') { route.name = undefined; route.params = {}; route.fullPath = value } else { route.name = value.name; route.params = value.params; route.fullPath = `/channel/${value.params.uid}` } } }
let confirmInput, routeLeave, renameFailure
const notices = []
const api = {
  keyboard: { capture: async (uid, enabled) => { calls.push(['keyboard', uid, enabled]); return { uid, active: enabled, error: '' } }, onState: () => () => {} },
  scan: async () => structuredClone(inventory), onInventory: () => () => {},
  state: async () => structuredClone(inventory),
  video: { start: async (uid, mode, requestId) => { calls.push(['video', uid]); return { uid, requestId, token: 1, width: 1920, height: 1080, frameRate: 30, frames: 1, dropped: 0 } }, stop: async () => { calls.push(['stop']) }, ack() {}, fullscreen: async enabled => { calls.push(['fullscreen', enabled]) }, onFrame: () => () => {}, onEnded: () => () => {} },
  tools: { deactivate: async uid => { calls.push(['deactivate', uid]) }, rename: async (uid, name) => { calls.push(['rename', uid, name]); if (renameFailure) throw Error(renameFailure); inventory.boards[0].channels.find(c => c.uid === uid).name = name } },
  releaseInput: async () => { calls.push(['release']) }, input: async (uid, input) => { calls.push(['input', uid, input]) }
}
global.window = { kbmNative: { mini: api }, addEventListener: (n, cb) => events.set(n, cb), removeEventListener: n => events.delete(n) }
global.document = { hidden: false, addEventListener() {}, removeEventListener() {} }
const settings = new Map(); global.localStorage = { getItem: k => settings.get(k) ?? null, setItem: (k, v) => settings.set(k, v) }
const tools = defineComponent({ setup(_, { expose }) { expose({ show: name => calls.push(['tool', name]) }); return () => h('tools') } })
function load(file) {
  if (file.endsWith('MiniChannelTools.vue')) return { __esModule: true, default: tools }
  if (file.endsWith('OperationLogs.vue')) return { __esModule: true, default: defineComponent({ render: () => null }) }
  if (cache.has(file)) return cache.get(file)
  let source = fs.readFileSync(file, 'utf8')
  if (file.endsWith('.vue')) { const { descriptor, errors } = sfc.parse(source, { filename: file }); assert.deepEqual(errors, []); source = sfc.compileScript(descriptor, { id: file, inlineTemplate: true }).content }
  const exports = {}; cache.set(file, exports)
  const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true } }).outputText
  vm.runInThisContext(`(function(exports,require){${code}\n})`, { filename: file })(exports, name => {
    if (name === 'vue') return vue
    if (name === 'vue-router') return { useRoute: () => route, useRouter: () => router, onBeforeRouteLeave: fn => { routeLeave = fn } }
    if (name === 'quasar') return { useQuasar: () => ({ notify: value => notices.push(value), dialog: () => ({ onOk: fn => { confirmInput = fn } }) }) }
    if (name.startsWith('.')) return load(path.resolve(path.dirname(file), name + (path.extname(name) ? '' : '.ts')))
    throw Error(`Unexpected import ${name}`)
  }); return exports
}
const node = (type, text = '') => ({ type, text, children: [], props: {}, parent: null, focus() { document.activeElement = this; this.props.onFocus?.() }, setPointerCapture() {}, getBoundingClientRect: () => ({ left: 0, top: 0, width: 1000, height: 1000 }) })
const renderer = createRenderer({
  createElement: type => node(type), createText: text => node('#text', text), createComment: text => node('#comment', text),
  setText: (n, text) => { n.text = text }, setElementText: (n, text) => { n.text = text; n.children = [] },
  patchProp: (n, k, _old, v) => { n.props[k] = v }, parentNode: n => n.parent,
  nextSibling: n => { const list = n.parent?.children ?? []; return list[list.indexOf(n) + 1] ?? null },
  insert: (n, parent, anchor) => { if (n.parent) n.parent.children.splice(n.parent.children.indexOf(n), 1); n.parent = parent; const at = anchor ? parent.children.indexOf(anchor) : -1; parent.children.splice(at < 0 ? parent.children.length : at, 0, n) },
  remove: n => { if (n.parent) n.parent.children.splice(n.parent.children.indexOf(n), 1); n.parent = null }
})
function walk(n) { return [n, ...n.children.flatMap(walk)] }
const settle = async () => { for (let n = 0; n < 24; n++) { await Promise.resolve(); await nextTick() } }
async function main() {
  const app = renderer.createApp(load(path.join(root, 'kbm/src/pages/MiniKmbPage.vue')).default), container = node('root')
  for (const name of ['layout', 'header', 'toolbar', 'toolbar-title', 'chip', 'btn', 'menu', 'list', 'item', 'item-section', 'item-label', 'drawer', 'page-container', 'page', 'banner', 'card', 'card-section', 'card-actions', 'icon', 'space', 'select', 'checkbox']) {
    app.component(`q-${name}`, defineComponent({ inheritAttrs: false, setup(_, ctx) { return () => h(name === 'btn' ? 'button' : 'div', { ...ctx.attrs, disabled: Boolean(ctx.attrs.disable), 'data-q': name }, [ctx.attrs.label ?? '', ...(ctx.slots.default?.() ?? [])]) } }))
  }
  const button = label => { const b = walk(container).find(n => n.type === 'button' && n.props.label === label); assert(b, label); return b }
  const click = async b => { assert(!b.props.disabled); await b.props.onClick?.({ stopPropagation() {} }); await settle() }
  app.mount(container)
  try {
    await settle(); assert.equal(calls.filter(c => c[0] === 'video').length, 0, 'manager must not open a capture')
    assert(walk(container).some(n => n.text === `0x${UID}`), 'manager renders hexadecimal UID with prefix')
    await click(button('제어하기')); assert.equal(route.params.uid, UID)
    assert(!calls.some(c => c[0] === 'fullscreen'), 'control keeps normal window mode')
    assert.deepEqual(calls.filter(c => c[0] === 'video'), [['video', UID]])
    assert(walk(container).some(n => n.text.includes(`0x${UID} ·`)), 'top bar renders hexadecimal UID while transport keeps its original UID')
    assert(button('채널 목록')); await click(button('시리얼 중계')); assert.deepEqual(calls.at(-1), ['tool', 'serial'])
    assert.equal(calls.filter(c => c[0] === 'input').length, 0, 'navigation is never HID input')
    assert(!walk(container).some(n => n.type === 'button' && n.props.label === '입력 허용'))
    assert(!walk(container).some(n => n.props.class === 'mini-logo' || n.props.class === 'mini-footer' || n.props.class === 'mini-overlay'))
    const header = walk(container).find(n => n.props['data-q'] === 'header'); assert(header)
    assert(['영상 열기', '중지', '수동 W/H/Hz'].every(label => walk(header).includes(button(label))), 'video controls must be in the top bar')
    assert(walk(header).some(n => n.props.label === '영상 모드'))
    assert(!walk(container).some(n => ['mini-controls', 'mini-channel-heading', 'mini-help'].includes(n.props.class)), 'no extra video overlay cards')
    assert(!walk(container).some(n => n.text.includes('동시 영상 1채널')))
    await click(button('이름 변경')); await confirmInput('서버 A'); await settle(); assert.deepEqual(calls.at(-1), ['rename', UID, '서버 A'])
    assert(walk(container).some(n => n.text.includes('서버 A')), 'SD name is shown in current-channel title')
    assert.equal(notices.at(-1).type, 'positive')
    renameFailure = 'SD 작업이 진행 중입니다.'
    await click(button('이름 변경')); await confirmInput('다른 이름'); await settle()
    assert.equal(notices.at(-1).type, 'negative'); assert(notices.at(-1).message.includes(renameFailure))
    assert.equal(inventory.boards[0].channels[0].name, '서버 A', 'failed saves never display the requested name as saved')
    renameFailure = undefined
    const stage = walk(container).find(n => typeof n.props.onPointerdown === 'function'); assert(stage)
    const event = (x, y, button = 0) => ({ clientX: x, clientY: y, pointerId: 1, button, preventDefault() {} })
    stage.props.onPointerdown(event(50, 50)); await settle(); assert.equal(calls.filter(c => c[0] === 'input').length, 0, 'letterbox click ignored')
    stage.props.onPointerdown(event(500, 500)); stage.props.onPointermove(event(900, 650)); stage.props.onPointerup(event(1000, 1000)); await settle()
    const input = calls.filter(c => c[0] === 'input')
    assert(input.every(c => c[1] === UID)); assert.deepEqual(input[0][2], { kind: 'move', x: 16384, y: 16384 })
    assert.deepEqual(input[1][2], { kind: 'button', button: 1, down: true }); assert.deepEqual(input.at(-1)[2], { kind: 'button', button: 1, down: false })
    assert(input.some(c => c[2].kind === 'move' && c[2].x === 32767 && c[2].y === 32767))
    const beforeBlur = calls.filter(c => c[0] === 'input').length; events.get('blur')(); stage.props.onPointerdown(event(500, 500)); await settle(); assert.equal(calls.filter(c => c[0] === 'input').length, beforeBlur)
    events.get('focus')(); stage.props.onPointerdown(event(500, 500)); stage.props.onPointerup(event(500, 500)); await settle(); assert(calls.filter(c => c[0] === 'input').length > beforeBlur, 'focus return needs no unlock');
    stage.props.onKeydown({ code: 'Escape', repeat: false, preventDefault() {} }); stage.props.onKeyup({ code: 'Escape', preventDefault() {} }); await settle(); assert(calls.some(c => c[0] === 'input' && c[2].kind === 'key' && c[2].usage === 0x29), 'Escape is a target key');
    let special = walk(container).find(n => n.props.label === '특수 키 전송')
    const key = (code, key) => ({ code, key, repeat: false, preventDefault() {} })
    let start = calls.length; stage.props.onKeydown(key('MetaLeft', 'Meta')); stage.props.onKeydown(key('AltRight', 'HangulMode')); await settle()
    assert(!calls.slice(start).some(c => c[0] === 'input'), 'special keys start OFF')
    await special.props['onUpdate:modelValue'](true); await settle()
    assert(calls.some(c => c[0] === 'keyboard' && c[1] === UID && c[2] === true), 'native host suppression activates only for selected video')
    start = calls.length
    stage.props.onKeydown(key('MetaLeft', 'Meta')); stage.props.onKeyup(key('MetaLeft', 'Meta'))
    stage.props.onKeydown(key('AltRight', 'HangulMode')); stage.props.onKeyup(key('AltRight', 'AltGraph')); await settle()
    assert(!calls.slice(start).some(c => c[0] === 'input'), 'DOM must not duplicate native-captured input')
    stage.props.onBlur(); await settle(); assert(calls.some(c => c[0] === 'keyboard' && c[2] === false))
    stage.props.onFocus(); await settle(); assert.equal(calls.filter(c => c[0] === 'keyboard').at(-1)[2], true, 'capture resumes only on video focus')
    start = calls.length; await special.props['onUpdate:modelValue'](false); await settle()
    assert(calls.slice(start).some(c => c[0] === 'release')); assert.equal(calls.filter(c => c[0] === 'keyboard').at(-1)[2], false)
    await click(button('가상 키보드 열기')); assert.deepEqual(calls.at(-1), ['tool', 'keyboard'])
    await click(button('채널 목록')); assert.equal(route.fullPath, '/'); assert(calls.some(c => c[0] === 'deactivate' && c[1] === UID))
    assert(!calls.some(c => c[0] === 'fullscreen'))
    const cards = walk(container).filter(n => n.type === 'button' && n.props.label === '제어하기'); await click(cards[1]); assert.equal(route.params.uid, OTHER); assert.deepEqual(calls.filter(c => c[0] === 'video').at(-1), ['video', OTHER])
    assert.equal(calls.filter(c => c[0] === 'input' && c[1] === OTHER).length, 0)
    await routeLeave(); await router.push({ name: 'mini-control', params: { uid: 'MISSING' } }); await settle()
    assert.deepEqual(calls.filter(c => c[0] === 'video').at(-1), ['video', OTHER], 'missing UID never selects a different channel')
    console.log('PASS real Vue manager→Control console, selected UID tools, direct input without unlock, absolute click/drag, blur release/automatic resume, channel return/switch and missing-UID isolation; no hardware input')
  } finally { app.unmount(); await settle() }
}
main().catch(e => { console.error(e); process.exitCode = 1 })
