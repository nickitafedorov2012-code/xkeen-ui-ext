export const SERVERS_COLUMNS_KEY = 'xr_servers_columns'
export const SERVERS_PAGE_SIZE = 24

export function loadServersColumns(): number {
  try {
    const saved = localStorage.getItem(SERVERS_COLUMNS_KEY)
    if (saved) {
      const n = parseInt(saved, 10)
      if (n >= 0 && n <= 4) return n
    }
  } catch {
    // ignore storage errors
  }
  return 0
}

export function saveServersColumns(cols: number): void {
  try {
    localStorage.setItem(SERVERS_COLUMNS_KEY, String(cols))
  } catch {
    // ignore storage errors
  }
}

export function isWhitelistToggleVisible(
  servers: Array<{ is_whitelist?: boolean }>,
  whitelistProviders?: string[]
): boolean {
  return Boolean(
    servers.some((s) => s.is_whitelist) ||
      (whitelistProviders && whitelistProviders.length > 0)
  )
}

export function isDeviceActionDisabled(device: { ip?: string; online?: boolean }): boolean {
  return !device.ip || !device.online
}

export interface SignalBarsInfo {
  active: number
  status: 'timeout' | 'excellent' | 'good' | 'medium' | 'slow'
  color?: string
  label?: string
}

export function getSignalBarsInfo(ping: number): SignalBarsInfo {
  if (ping <= 0 || ping >= 9999) {
    return { active: 0, status: 'timeout' }
  }
  if (ping < 80) {
    return { active: 4, status: 'excellent', color: '#22c55e', label: 'Отличное (4/4)' }
  }
  if (ping < 160) {
    return { active: 3, status: 'good', color: '#38bdf8', label: 'Хорошее (3/4)' }
  }
  if (ping < 300) {
    return { active: 2, status: 'medium', color: '#f59e0b', label: 'Среднее (2/4)' }
  }
  return { active: 1, status: 'slow', color: '#ef4444', label: 'Слабое (1/4)' }
}

export function getWhitelistButtonLabels(columns: number, isWhitelisted: boolean) {
  const isMultiCol = columns === 0 || columns >= 2
  const full = isWhitelisted ? 'В белом списке' : 'Белый список'
  const short = isWhitelisted ? 'БС ON' : 'БС'
  return {
    full,
    short,
    effectiveDisplay: isMultiCol ? short : full,
  }
}
