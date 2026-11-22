export const UsbWakeCode = {
  Sent: 0, IgnoredActive: 1, IgnoredHostDisabled: 2, IgnoredDisconnected: 3,
  IgnoredTransition: 4, IgnoredUnsupported: 5, ProcessingTimeout: 6
} as const
export interface WakeResult { code: number }
export const WolCode = { SentUnconfirmed: 128, IgnoredDisabled: 129, IgnoredUnsupported: 130 } as const
export interface WolConfig { enabled: boolean; mac: string; localAddress: string }
export interface WolAdapter { name: string; address: string; broadcast: string }
export const defaultWolConfig = (): WolConfig => ({ enabled: false, mac: '', localAddress: '' })
export function normalizeWakeMac(value: string): string {
  if (!/^(?:[0-9a-f]{12}|(?:[0-9a-f]{2}:){5}[0-9a-f]{2}|(?:[0-9a-f]{2}-){5}[0-9a-f]{2})$/i.test(value.trim())) throw new Error('MAC 주소 형식이 잘못되었습니다.')
  const raw = value.trim().replace(/[:-]/g, '').toLowerCase()
  if ((parseInt(raw.slice(0, 2), 16) & 1) || /^0{12}$/.test(raw)) throw new Error('대상 PC의 유니캐스트 유선 LAN MAC 주소가 필요합니다.')
  return raw.match(/../g)!.join(':')
}
export function wakeResultText(code: number): string {
  return ({ 0: 'USB 깨우기 신호 발신 (기상 확인 불가)', 1: '무시: USB가 이미 활성 상태',
    2: '무시: 호스트가 USB 깨우기를 허용하지 않음', 3: '무시: USB 미연결',
    4: '무시: USB 재연결 중', 5: '무시: 기능 미지원/스택 거부', 6: 'USB 처리기한 만료 (결과 확인 불가)',
    128: 'WOL 패킷 발신 (지원 여부·기상 확인 불가)', 129: '무시: WOL 비활성',
    130: '무시: 이 시스템의 UDP 브로드캐스트 미지원' } as Record<number, string>)[code] ?? `알 수 없는 결과 ${code}`
}
