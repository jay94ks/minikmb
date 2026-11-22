import { keyboardCodeToUsage } from '../services/hid-keyboard'
export interface KeyChordRequest { modifiers: number; key: string }
export interface SpecialInputState { busy: boolean; needsRelease: boolean }
export const specialKeyOptions = [
  ...Array.from({ length: 26 }, (_, n) => ({ label: String.fromCharCode(65 + n), value: `Key${String.fromCharCode(65 + n)}` })),
  ...Array.from({ length: 10 }, (_, n) => ({ label: String(n), value: `Digit${n}` })),
  ...Array.from({ length: 24 }, (_, n) => ({ label: `F${n + 1}`, value: `F${n + 1}` })),
  ...['Delete', 'Insert', 'Home', 'End', 'PageUp', 'PageDown', 'Enter', 'Tab', 'Escape',
    'Space', 'Backspace', 'PrintScreen', 'Pause', 'ContextMenu', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight',
    'Backquote', 'Minus', 'Equal', 'BracketLeft', 'BracketRight', 'Backslash', 'Semicolon', 'Quote',
    'Comma', 'Period', 'Slash', 'CapsLock', 'ScrollLock', 'NumLock', 'Lang1', 'Lang2']
    .map(value => ({ label: value, value }))
]
export const specialPresets = [
  { label: 'Ctrl + Alt + Delete', modifiers: 3, key: 'Delete' },
  { label: 'Windows 키', modifiers: 8, key: '' },
  { label: 'Win + L', modifiers: 8, key: 'KeyL' },
  { label: 'Win + D', modifiers: 8, key: 'KeyD' },
  { label: 'Win + E', modifiers: 8, key: 'KeyE' },
  { label: 'Win + R', modifiers: 8, key: 'KeyR' },
  { label: 'Win + Tab', modifiers: 8, key: 'Tab' },
  { label: 'Alt + Tab', modifiers: 2, key: 'Tab' },
  { label: 'Alt + F4', modifiers: 2, key: 'F4' },
  { label: 'Ctrl + Shift + Esc', modifiers: 5, key: 'Escape' }
] as const
export function chordUsages(value: KeyChordRequest): number[] {
  if (!value || !Number.isInteger(value.modifiers) || value.modifiers < 0 || value.modifiers > 15 || typeof value.key !== 'string') throw new Error('Invalid special key request')
  const keys = [0xe0, 0xe2, 0xe1, 0xe3].filter((_, index) => value.modifiers & (1 << index))
  if (value.key) {
    if (!specialKeyOptions.some(option => option.value === value.key)) throw new Error('Unsupported special key')
    const usage = keyboardCodeToUsage(value.key)
    if (usage === undefined) throw new Error('Unsupported special key')
    keys.push(usage)
  }
  if (!keys.length) throw new Error('전송할 키를 선택하세요.')
  return keys
}
