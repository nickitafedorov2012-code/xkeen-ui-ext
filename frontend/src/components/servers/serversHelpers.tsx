import type { ServerInfo } from '../../types'

export function Sparkline({ ping }: { ping: number }) {
  // 4 полосы качества сигнала Apple-style (высоты: 4px, 7px, 10px, 14px)
  const barHeights = [4, 7, 10, 14]

  if (ping <= 0 || ping >= 9999) {
    return (
      <div
        className="signal-bars-meter timeout"
        title="Таймаут соединения: сервер не отвечает на ICMP/TCP пинг (оффлайн)"
      >
        <span className="signal-dot timeout" />
        <div className="signal-bars-wrap">
          {barHeights.map((h, idx) => (
            <span
              key={idx}
              className="signal-bar bar-timeout"
              style={{ height: h }}
            />
          ))}
        </div>
      </div>
    )
  }

  // Оценка качества по пингу:
  // < 80 мс: 4/4 (зеленый)
  // 80..160 мс: 3/4 (голубой)
  // 160..300 мс: 2/4 (янтарный)
  // >= 300 мс: 1/4 (красный)
  const activeBars = ping < 80 ? 4 : ping < 160 ? 3 : ping < 300 ? 2 : 1
  const qualityColor =
    activeBars === 4 ? '#22c55e' : activeBars === 3 ? '#38bdf8' : activeBars === 2 ? '#f59e0b' : '#ef4444'
  const qualityLabel =
    activeBars === 4
      ? 'Отличное (4/4)'
      : activeBars === 3
      ? 'Хорошее (3/4)'
      : activeBars === 2
      ? 'Среднее (2/4)'
      : 'Слабое (1/4)'

  return (
    <div
      className="signal-bars-meter"
      title={`Качество связи: ${qualityLabel} • Пинг: ${ping} мс • Стабильное соединение`}
    >
      <span className="signal-dot" style={{ backgroundColor: qualityColor, boxShadow: `0 0 6px ${qualityColor}` }} />
      <div className="signal-bars-wrap">
        {barHeights.map((h, idx) => {
          const isActive = idx < activeBars
          return (
            <span
              key={idx}
              className={`signal-bar ${isActive ? 'active' : 'dimmed'}`}
              style={{
                height: h,
                backgroundColor: isActive ? qualityColor : undefined,
              }}
            />
          )
        })}
      </div>
    </div>
  )
}

export const PROVIDER_COLORS = [
  '#22c55e', // green
  '#a855f7', // purple
  '#38bdf8', // sky blue
  '#f59e0b', // amber
  '#ec4899', // pink
  '#14b8a6', // teal
  '#6366f1', // indigo
]

export function formatSpeed(bytesPerSec: number): string {
  if (!bytesPerSec || bytesPerSec <= 0) return '0 Б/с'
  if (bytesPerSec < 1024) return `${bytesPerSec} Б/с`
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} КБ/с`
  if (bytesPerSec < 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} МБ/с`
  return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(2)} ГБ/с`
}

export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 Б'
  if (bytes < 1024) return `${bytes} Б`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} КБ`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} МБ`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} ГБ`
}

export function formatQuotaBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 Б'
  if (bytes < 1024) return `${bytes} Б`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} КБ`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} МБ`

  // Поддержка как десятичных (1000^3/1000^4), так и двоичных (1024^3/1024^4) квот провайдеров
  // Если число кратно или рассчитано в десятичных ГБ (например 700 * 1e9 или 2 * 1e12)
  const isDecimalGb = bytes % 1000000000 === 0 && bytes % 1073741824 !== 0
  const isDecimalTb = bytes % 1000000000000 === 0 && bytes % (1024 * 1024 * 1024 * 1024) !== 0
  if (isDecimalTb) {
    return `${(bytes / 1000000000000).toFixed(2)} ТБ`
  }
  if (isDecimalGb) {
    return `${(bytes / 1000000000).toFixed(2)} ГБ`
  }

  if (bytes < 1024 * 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} ГБ`
  return `${(bytes / (1024 * 1024 * 1024 * 1024)).toFixed(2)} ТБ`
}

export function isPoolServer(s: ServerInfo): boolean {
  if (s.is_pool) return true
  const proto = (s.protocol || '').toUpperCase()
  if (proto.includes('POOL') || proto === 'URL-TEST' || proto === 'FALLBACK' || proto === 'SELECTOR' || proto === 'LOAD-BALANCE') {
    return true
  }
  if (s.id === 'Fastest' || s.id === 'Fallback' || s.id === 'PROXY' || s.id === 'GLOBAL') {
    return true
  }
  return false
}
