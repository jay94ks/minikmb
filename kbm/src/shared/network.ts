export type NetworkPort = 0 | 1
export interface PortScanRequest { target: string; start: number; end: number; timeoutMs: number }
export interface PortScanState { running: boolean; target: string; start: number; end: number; completed: number; total: number; open: number[]; closed: number; filtered: number; error: string }
export interface NetworkConfig {
  serverIp: string
  poolStart: number
  poolEnd: number
  leaseSeconds: number
}
export interface ForwardRule {
  id: string
  port: NetworkPort
  protocol: 'tcp' | 'udp'
  listenPort: number
  targetIp: string
  targetPort: number
}
export interface NetworkState {
  enabled: [boolean, boolean]
  forwarding: boolean
  rxFrames: [number, number]
  txFrames: [number, number]
  dropped: [number, number]
  queued: [number, number]
  leases: Array<{ port: NetworkPort; ip: string; mac: string }>
  connections: number
  error: string
}
export const defaultNetworkConfig: NetworkConfig = {
  serverIp: '192.168.127.1', poolStart: 100, poolEnd: 199, leaseSeconds: 3600
}
export function validIp(ip: unknown): ip is string {
  return typeof ip === 'string' && /^(\d{1,3}\.){3}\d{1,3}$/.test(ip) &&
    ip.split('.').every(octet => Number(octet) <= 255 && String(Number(octet)) === octet)
}
export function validateNetworkConfig(config: NetworkConfig): void {
  if (!config || !validIp(config.serverIp)) throw new Error('올바른 서버 IPv4 주소가 필요합니다.')
  const octets = config.serverIp.split('.').map(Number)
  if (octets[3] !== 1) throw new Error('Master/게이트웨이 주소는 서브넷의 .1이어야 합니다.')
  if (!(octets[0] === 10 || (octets[0] === 172 && octets[1]! >= 16 && octets[1]! <= 31) ||
    (octets[0] === 192 && octets[1] === 168))) throw new Error('서버 주소는 사설 IPv4 범위여야 합니다.')
  if (!Number.isInteger(config.poolStart) || !Number.isInteger(config.poolEnd) ||
    config.poolStart < 2 || config.poolEnd > 254 || config.poolStart >= config.poolEnd ||
    octets[3]! >= config.poolStart && octets[3]! <= config.poolEnd) {
    throw new Error('DHCP 범위(2..254)는 서버 주소를 제외하고 최소 2개 주소를 포함해야 합니다.')
  }
  if (!Number.isInteger(config.leaseSeconds) || config.leaseSeconds < 60 || config.leaseSeconds > 86400) {
    throw new Error('DHCP 임대 시간은 60..86400초입니다.')
  }
}
export function validateForwardRule(rule: ForwardRule, config: NetworkConfig): void {
  const prefix = config.serverIp.slice(0, config.serverIp.lastIndexOf('.') + 1)
  if (!rule || (rule.port !== 0 && rule.port !== 1) || !['tcp', 'udp'].includes(rule.protocol) ||
      !validIp(rule.targetIp) || !rule.targetIp.startsWith(prefix) || rule.targetIp === config.serverIp ||
      ['0', '255'].includes(rule.targetIp.split('.')[3]!) ||
      ![rule.listenPort, rule.targetPort].every(p => Number.isInteger(p) && p >= 1 && p <= 65535)) {
    throw new Error('포워딩 대상은 지정한 slave의 /24 주소와 유효한 TCP/UDP 포트여야 합니다.')
  }
}
