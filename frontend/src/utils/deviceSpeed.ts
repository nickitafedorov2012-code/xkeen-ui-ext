export function kbpsToMbpsInput(kbps?: number): string {
  if (!kbps || kbps <= 0) return ''
  const mbps = kbps / 1024
  return kbps % 1024 === 0 ? String(mbps) : mbps.toFixed(1)
}

export function mbpsInputToKbps(val: string): number {
  const trimmed = val.trim()
  if (!trimmed || trimmed === '0') return 0
  const num = parseFloat(trimmed.replace(',', '.'))
  if (!isNaN(num) && num > 0) {
    return Math.round(num * 1024)
  }
  return 0
}
