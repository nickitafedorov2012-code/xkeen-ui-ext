export interface DashPingPoint {
  ms: number
  ok: boolean
}

export function calcJitter(history: number[]): number {
  if (history.length < 2) return 0
  let sum = 0
  for (let i = 1; i < history.length; i++) {
    sum += Math.abs(history[i] - history[i - 1])
  }
  return Math.round(sum / (history.length - 1))
}

export function getJitterColor(jitter: number, pointsCount: number): string {
  if (pointsCount < 2) return 'var(--muted)'
  if (jitter > 25) return '#ef4444'
  if (jitter > 10) return '#eab308'
  return '#22c55e'
}

export function loadServerPingHistory(serverId: string): number[] {
  try {
    const raw = sessionStorage.getItem(`xr_pings_${serverId}`)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed : []
  } catch {
    return []
  }
}

export function saveServerPingHistory(serverId: string, history: number[]): void {
  try {
    sessionStorage.setItem(`xr_pings_${serverId}`, JSON.stringify(history))
  } catch {
    // ignore storage errors
  }
}

export function loadDashPingHistory(serverId: string): DashPingPoint[] {
  try {
    const raw = sessionStorage.getItem(`xr_dash_pings_${serverId}`)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed : []
  } catch {
    return []
  }
}

export function saveDashPingHistory(serverId: string, history: DashPingPoint[]): void {
  try {
    sessionStorage.setItem(`xr_dash_pings_${serverId}`, JSON.stringify(history))
  } catch {
    // ignore storage errors
  }
}
