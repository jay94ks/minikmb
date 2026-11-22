import { keyboardCodeToUsage } from './hid-keyboard'

export function miniKeyboardUsage(event: { code: string; key?: string }): number | undefined {
  if (event.key === 'HangulMode' || event.key === 'Hangul') return 0x90
  if (event.key === 'HanjaMode' || event.key === 'Hanja') return 0x91
  return keyboardCodeToUsage(event.code)
}

export function isMiniSpecialKey(usage: number): boolean {
  return usage === 0xe3 || usage === 0xe7 || usage === 0x65 ||
    (usage >= 0x46 && usage <= 0x48) || (usage >= 0x87 && usage <= 0x98)
}
