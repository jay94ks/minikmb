// Offline, pure TypeScript evaluation only. No HID, hooks, USB or network.
// Run from any directory: node kbm-rs/scripts/export-keyboard-fixture.cjs
const fs = require('node:fs')
const path = require('node:path')
const vm = require('node:vm')
const crypto = require('node:crypto')
const root = path.resolve(__dirname, '../..')
const ts = require(path.join(root, 'kbm/node_modules/typescript'))
const sources = ['kbm/src/services/hid-keyboard.ts', 'kbm/src/services/mini-keyboard.ts']
const text = sources.map(name => fs.readFileSync(path.join(root, name), 'utf8'))
const loaded = []
for (let i = 0; i < sources.length; i++) {
  const exports = {}
  const requireLocal = name => {
    if (i === 1 && name === './hid-keyboard') return loaded[0]
    throw Error(`Unexpected fixture dependency: ${name}`)
  }
  const js = ts.transpileModule(text[i], {compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022}}).outputText
  vm.runInNewContext(js, {exports, require: requireLocal}, {filename: sources[i], timeout: 1000})
  loaded.push(exports)
}
const ast = ts.createSourceFile(sources[0], text[0], ts.ScriptTarget.ES2022, true)
let fixed
function visit(node) {
  if (ts.isVariableDeclaration(node) && node.name.getText(ast) === 'fixedUsages') {
    if (!ts.isObjectLiteralExpression(node.initializer)) throw Error('fixedUsages shape changed')
    fixed = node.initializer.properties.map(p => p.name.getText(ast))
  }
  ts.forEachChild(node, visit)
}
visit(ast)
if (!fixed) throw Error('Missing v1 fixedUsages')
const windows = {}
const add = (code, vk, scan, extended = false) => {windows[code] = {vk, scan, extended}}
for (const [letters, start] of [['QWERTYUIOP', 0x10], ['ASDFGHJKL', 0x1e], ['ZXCVBNM', 0x2c]]) {
  [...letters].forEach((letter, i) => add(`Key${letter}`, letter.charCodeAt(0), start + i))
}
for (let n = 0; n <= 9; n++) add(`Digit${n}`, 0x30 + n, n === 0 ? 0x0b : n + 1)
for (let n = 1; n <= 24; n++) {
  const scan = n <= 10 ? 0x3a + n : n <= 12 ? 0x4c + n : n === 24 ? 0x76 : 0x57 + n
  add(`F${n}`, 0x6f + n, scan)
}
for (const [code, vk, scan] of [
  ['Enter', 0x0d, 0x1c], ['Escape', 0x1b, 0x01], ['Backspace', 8, 0x0e], ['Tab', 9, 0x0f],
  ['Space', 0x20, 0x39], ['Minus', 0xbd, 0x0c], ['Equal', 0xbb, 0x0d], ['BracketLeft', 0xdb, 0x1a],
  ['BracketRight', 0xdd, 0x1b], ['Backslash', 0xdc, 0x2b], ['Semicolon', 0xba, 0x27], ['Quote', 0xde, 0x28],
  ['Backquote', 0xc0, 0x29], ['Comma', 0xbc, 0x33], ['Period', 0xbe, 0x34], ['Slash', 0xbf, 0x35],
  ['CapsLock', 0x14, 0x3a], ['ScrollLock', 0x91, 0x46], ['Pause', 0x13, 0x45], ['NumLock', 0x90, 0x45],
  ['NumpadMultiply', 0x6a, 0x37], ['NumpadSubtract', 0x6d, 0x4a], ['NumpadAdd', 0x6b, 0x4e],
  ['NumpadDecimal', 0x6e, 0x53], ['IntlBackslash', 0xe2, 0x56], ['IntlRo', 0xe2, 0x73], ['IntlYen', 0xdc, 0x7d],
  ['ControlLeft', 0xa2, 0x1d], ['ShiftLeft', 0xa0, 0x2a], ['AltLeft', 0xa4, 0x38], ['ShiftRight', 0xa1, 0x36]
]) add(code, vk, scan)
for (const [code, vk, scan] of [
  ['PrintScreen', 0x2c, 0x37], ['Insert', 0x2d, 0x52], ['Home', 0x24, 0x47], ['PageUp', 0x21, 0x49],
  ['Delete', 0x2e, 0x53], ['End', 0x23, 0x4f], ['PageDown', 0x22, 0x51], ['ArrowRight', 0x27, 0x4d],
  ['ArrowLeft', 0x25, 0x4b], ['ArrowDown', 0x28, 0x50], ['ArrowUp', 0x26, 0x48], ['NumpadDivide', 0x6f, 0x35],
  ['NumpadEnter', 0x0d, 0x1c], ['ContextMenu', 0x5d, 0x5d], ['Lang1', 0x15, 0x38], ['Lang2', 0x19, 0x1d],
  ['MetaLeft', 0x5b, 0x5b], ['ControlRight', 0xa3, 0x1d], ['AltRight', 0xa5, 0x38], ['MetaRight', 0x5c, 0x5c]
]) add(code, vk, scan, true)
const numpad = [0x52, 0x4f, 0x50, 0x51, 0x4b, 0x4c, 0x4d, 0x47, 0x48, 0x49]
numpad.forEach((scan, n) => add(`Numpad${n}`, 0x60 + n, scan))
const codes = [...fixed, ...Array.from({length: 26}, (_, i) => `Key${String.fromCharCode(65 + i)}`),
  ...Array.from({length: 10}, (_, i) => `Digit${i}`), ...Array.from({length: 24}, (_, i) => `F${i + 1}`)]
if (new Set(codes).size !== codes.length) throw Error('Duplicate DOM code')
const keys = codes.map(code => {
  const usage = loaded[0].keyboardCodeToUsage(code)
  if (!windows[code] || !Number.isInteger(usage)) throw Error(`Missing mapping: ${code}`)
  return {code, ...windows[code], usage, special: loaded[1].isMiniSpecialKey(usage)}
})
const result = {
  sources: sources.map((name, i) => ({path: name, sha256: crypto.createHash('sha256').update(text[i]).digest('hex')})),
  windows_reference: 'https://learn.microsoft.com/en-us/windows/win32/inputdev/about-keyboard-input',
  note: 'Expected usage/special values executed from Electron v1; Windows VK/Scan1/E0 samples are independently specified. No keyboard injection.',
  keys
}
const output = path.join(root, 'kbm-rs/tests/keyboard-dom-v1.json')
fs.writeFileSync(output, JSON.stringify(result, null, 2) + '\n')
console.log(`Exported ${keys.length} DOM keys to ${path.relative(root, output)}`)

// Capture the actual v1 custom selector and chord order, not a Rust-derived list.
const specialPath = 'kbm/src/shared/special-input.ts'
const specialText = fs.readFileSync(path.join(root, specialPath), 'utf8')
const special = {}
vm.runInNewContext(ts.transpileModule(specialText, {compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022}}).outputText,
  {exports: special, require: name => {
    if (name === '../services/hid-keyboard') return loaded[0]
    throw Error(`Unexpected chord fixture dependency: ${name}`)
  }}, {filename: specialPath, timeout: 1000})
const options = special.specialKeyOptions.map(option => ({
  label: option.label,
  usage: loaded[0].keyboardCodeToUsage(option.value),
  combinations: Array.from({length:16}, (_, modifiers) => ({modifiers, usages:special.chordUsages({modifiers,key:option.value})}))
}))
const chordOutput = path.join(root, 'kbm-rs/tests/keyboard-chords-v1.json')
fs.writeFileSync(chordOutput, JSON.stringify({source:specialPath, sha256:crypto.createHash('sha256').update(specialText).digest('hex'),
  note:'Executed from Electron v1 pure functions only; no HID or device access.', options}, null, 2) + '\n')
console.log(`Exported ${options.length} custom keys and all modifier combinations to ${path.relative(root,chordOutput)}`)
