export interface VirtualKey { code: string; label: string; width?: number }
const key = (code: string, label = code, width = 1): VirtualKey => ({ code, label, width })
const letters = (text: string): VirtualKey[] => [...text].map(letter => key(`Key${letter}`, letter))
export const virtualKeyboardRows: VirtualKey[][] = [
  [key('Escape', 'Esc'), ...Array.from({ length: 12 }, (_, n) => key(`F${n + 1}`)), key('PrintScreen', 'PrtSc'), key('ScrollLock', 'ScrLk'), key('Pause')],
  [key('Backquote', '`'), ...[1,2,3,4,5,6,7,8,9,0].map(n => key(`Digit${n}`, String(n))), key('Minus', '-'), key('Equal', '='), key('Backspace', 'Backspace', 2)],
  [key('Tab', 'Tab', 1.5), ...letters('QWERTYUIOP'), key('BracketLeft', '['), key('BracketRight', ']'), key('Backslash', '\\', 1.5)],
  [key('CapsLock', 'Caps', 1.8), ...letters('ASDFGHJKL'), key('Semicolon', ';'), key('Quote', "'"), key('Enter', 'Enter', 2.2)],
  [...letters('ZXCVBNM'), key('Comma', ','), key('Period', '.'), key('Slash', '/'), key('ArrowUp', '↑'), key('Delete', 'Del'), key('Insert', 'Ins')],
  [key('Lang1', '한/영'), key('Lang2', '한자'), key('Space', 'Space', 5), key('ContextMenu', 'Menu'), key('Home'), key('End'), key('PageUp', 'PgUp'), key('PageDown', 'PgDn'), key('ArrowLeft', '←'), key('ArrowDown', '↓'), key('ArrowRight', '→')]
]
export const virtualModifiers = [{ label: 'Ctrl', mask: 1 }, { label: 'Alt', mask: 2 }, { label: 'Shift', mask: 4 }, { label: 'Win', mask: 8 }]
