// Generic Desktop page01 System Control inputs (USB-IF HUT1.7).
// Keep IDs/order in sync with common/system_controls.def; tested across both.
export const systemControls = [
  [0x81, 'System Power Down', '전원', true],
  [0x82, 'System Sleep', '전원', true],
  [0x83, 'System Wake Up', '전원', false],
  [0x84, 'System Context Menu', '메뉴', false],
  [0x85, 'System Main Menu', '메뉴', false],
  [0x86, 'System App Menu', '메뉴', false],
  [0x87, 'System Menu Help', '메뉴', false],
  [0x88, 'System Menu Exit', '메뉴', false],
  [0x89, 'System Menu Select', '메뉴', false],
  [0x8a, 'System Menu Right', '메뉴', false],
  [0x8b, 'System Menu Left', '메뉴', false],
  [0x8c, 'System Menu Up', '메뉴', false],
  [0x8d, 'System Menu Down', '메뉴', false],
  [0x8e, 'System Cold Restart', '전원', true],
  [0x8f, 'System Warm Restart', '전원', true],
  [0x97, 'System Function Shift', '기능', false],
  [0x98, 'System Function Shift Lock', '기능', false],
  [0x9a, 'System Dismiss Notification', '기능', false],
  [0x9b, 'System Do Not Disturb', '기능', false],
  [0xa0, 'System Dock', '기능', true],
  [0xa1, 'System Undock', '기능', true],
  [0xa2, 'System Setup', '기능', true],
  [0xa3, 'System Break', '디버그', true],
  [0xa4, 'System Debugger Break', '디버그', true],
  [0xa5, 'Application Break', '디버그', true],
  [0xa6, 'Application Debugger Break', '디버그', true],
  [0xa7, 'System Speaker Mute', '기능', false],
  [0xa8, 'System Hibernate', '전원', true],
  [0xa9, 'System Microphone Mute', '기능', false],
  [0xaa, 'System Accessibility Binding', '기능', false],
  [0xb0, 'System Display Invert', '화면', true],
  [0xb1, 'System Display Internal', '화면', true],
  [0xb2, 'System Display External', '화면', true],
  [0xb3, 'System Display Both', '화면', true],
  [0xb4, 'System Display Dual', '화면', true],
  [0xb5, 'System Display Toggle Int/Ext Mode', '화면', true],
  [0xb6, 'System Display Swap Primary/Secondary', '화면', true],
  [0xb7, 'System Display Toggle LCD Autoscale', '화면', true],
  [0xc9, 'System Display Rotation Lock Button', '화면', false],
  [0xca, 'System Display Rotation Lock Slider Switch', '화면', false]
].map(([usage, label, group, confirm]) => ({ usage: usage as number, label: label as string, group: group as string, confirm: confirm as boolean }))
export type SystemControl = typeof systemControls[number]
export type SystemControlStatus = 'accepted' | 'unsupported' | 'not-ready'
export interface SystemControlResult { status: SystemControlStatus; hostActionConfirmed: false; message: string }
export function systemControl(usage: number): SystemControl {
  const control = systemControls.find(item => item.usage === usage)
  if (!Number.isInteger(usage) || !control) throw new Error('지원하지 않는 System Control 입력입니다.')
  return control
}
