export function fmtUptime(secs?: number): string {
  if (!secs || secs <= 0) return '0 сек'
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h} ч ${m} мин`
  if (m > 0) return `${m} мин ${s} сек`
  return `${s} сек`
}

export function normalizeDomainInput(input: string): string {
  let s = input.trim().toLowerCase()
  s = s.replace(/^[a-z]+:\/\//i, '')
  s = s.split('/')[0].split('?')[0].split('#')[0]
  s = s.split('@').pop() || s
  s = s.split(':')[0]
  s = s.replace(/^www\./i, '')
  s = s.replace(/^\.+|\.+$/g, '')
  return s.trim()
}

export const renderMicroSpinner = (color = '#2563eb', size = 13) => (
    <svg
      className="zapret-spin"
      viewBox="0 0 24 24"
      fill="none"
      stroke={color}
      strokeWidth="3"
      strokeLinecap="round"
      style={{ width: size, height: size, display: 'block' }}
      data-testid="zapret-micro-spinner"
    >
      <circle cx="12" cy="12" r="9" stroke="currentColor" strokeOpacity="0.25" />
      <path d="M12 3a9 9 0 0 1 9 9" stroke="currentColor" />
    </svg>
  )