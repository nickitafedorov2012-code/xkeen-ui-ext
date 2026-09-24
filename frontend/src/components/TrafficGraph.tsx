import { useEffect, useState, useRef } from 'react'
import { apiGet } from '../api'

interface SpeedPair {
  down: number
  up: number
}

interface TrafficPayload {
  direct?: SpeedPair
  proxy?: SpeedPair
  total?: SpeedPair
  down?: number
  up?: number
}

interface TrafficHistoryItem {
  directDown: number
  directUp: number
  proxyDown: number
  proxyUp: number
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`
  if (bytesPerSec < 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`
  return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(2)} GB/s`
}

export default function TrafficGraph() {
  const [history, setHistory] = useState<TrafficHistoryItem[]>(() =>
    Array(30).fill({ directDown: 0, directUp: 0, proxyDown: 0, proxyUp: 0 })
  )

  const [currentDirect, setCurrentDirect] = useState<SpeedPair>({ down: 0, up: 0 })
  const [currentProxy, setCurrentProxy] = useState<SpeedPair>({ down: 0, up: 0 })
  const [peakDirect, setPeakDirect] = useState(0)
  const [peakProxy, setPeakProxy] = useState(0)

  // Режим отображения: 'down' (входящий) | 'up' (исходящий) | 'both' (сумма)
  const [metricMode, setMetricMode] = useState<'down' | 'up' | 'both'>('down')

  const activeRef = useRef(true)

  useEffect(() => {
    activeRef.current = true

    const poll = async () => {
      try {
        const data = await apiGet<TrafficPayload>('traffic/poll')
        if (!activeRef.current || !data) return

        const pDown = data.proxy?.down ?? data.down ?? 0
        const pUp = data.proxy?.up ?? data.up ?? 0
        const dDown = data.direct?.down ?? 0
        const dUp = data.direct?.up ?? 0

        setCurrentDirect({ down: dDown, up: dUp })
        setCurrentProxy({ down: pDown, up: pUp })

        setPeakDirect((prev) => Math.max(prev, dDown + dUp))
        setPeakProxy((prev) => Math.max(prev, pDown + pUp))

        setHistory((prev) => [
          ...prev.slice(1),
          {
            directDown: dDown,
            directUp: dUp,
            proxyDown: pDown,
            proxyUp: pUp,
          },
        ])
      } catch {
        /* игнорируем одиночные ошибки опроса */
      }
    }

    poll()
    const interval = setInterval(poll, 1000)
    return () => {
      activeRef.current = false
      clearInterval(interval)
    }
  }, [])

  // Вычисление отображаемого значения в зависимости от режима
  const getValue = (item: TrafficHistoryItem, type: 'direct' | 'proxy') => {
    if (type === 'direct') {
      if (metricMode === 'down') return item.directDown
      if (metricMode === 'up') return item.directUp
      return item.directDown + item.directUp
    } else {
      if (metricMode === 'down') return item.proxyDown
      if (metricMode === 'up') return item.proxyUp
      return item.proxyDown + item.proxyUp
    }
  }

  // Максимальное значение для масштабирования графика
  const maxVal = Math.max(
    ...history.map((h) => Math.max(getValue(h, 'direct'), getValue(h, 'proxy'))),
    1024 * 32 // Минимум 32 KB/s для красивой сетки в покое
  )

  const width = 640
  const height = 130
  const step = width / (history.length - 1)

  const getPoints = (type: 'direct' | 'proxy') => {
    return history.map((d, i) => {
      const x = i * step
      const val = getValue(d, type)
      const y = height - (val / maxVal) * (height - 18) - 6
      return { x, y }
    })
  }

  const directPoints = getPoints('direct')
  const proxyPoints = getPoints('proxy')

  const lineD = (pts: { x: number; y: number }[]) =>
    pts.reduce((acc, p, i) => `${acc} ${i === 0 ? 'M' : 'L'} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`, '')

  const areaD = (pts: { x: number; y: number }[]) => {
    const l = lineD(pts)
    return `${l} L ${width} ${height} L 0 ${height} Z`
  }

  const totalDown = currentDirect.down + currentProxy.down
  const totalUp = currentDirect.up + currentProxy.up

  return (
    <div className="traffic-graph-card card">
      <div className="traffic-graph-header">
        <div className="traffic-graph-title">
          <span className="traffic-icon">📈</span>
          <div>
            <h3 style={{ margin: 0, fontSize: 15 }}>Трафик в реальном времени</h3>
            <div className="traffic-legend-chips">
              <span className="legend-chip direct">
                <span className="legend-dot blue" /> Прямой трафик (Direct)
              </span>
              <span className="legend-chip proxy">
                <span className="legend-dot green" /> Проксированный (Proxy)
              </span>
            </div>
          </div>
        </div>

        {/* Переключатель режима: Загрузка / Отдача / Сумма */}
        <div className="traffic-controls">
          <div className="view-toggle" style={{ marginRight: 8 }}>
            <button
              type="button"
              className={`view-toggle-btn ${metricMode === 'down' ? 'active' : ''}`}
              onClick={() => setMetricMode('down')}
              title="Показывать только входящий трафик (загрузка)"
            >
              ↓ Загрузка
            </button>
            <button
              type="button"
              className={`view-toggle-btn ${metricMode === 'up' ? 'active' : ''}`}
              onClick={() => setMetricMode('up')}
              title="Показывать только исходящий трафик (отдача)"
            >
              ↑ Отдача
            </button>
            <button
              type="button"
              className={`view-toggle-btn ${metricMode === 'both' ? 'active' : ''}`}
              onClick={() => setMetricMode('both')}
              title="Показывать суммарную скорость (загрузка + отдача)"
            >
              ↓+↑ Сумма
            </button>
          </div>

          <div className="traffic-stats">
            {/* Синий бейдж: Прямой трафик */}
            <div className="traffic-badge direct" title={`Прямой трафик роутера (мимо прокси)\nПик: ${formatSpeed(peakDirect)}`}>
              <span className="badge-label">🔵 Прямой:</span>
              <span className="badge-arrow">↓</span>
              <span className="badge-val">{formatSpeed(currentDirect.down)}</span>
              <span className="badge-arrow">↑</span>
              <span className="badge-val">{formatSpeed(currentDirect.up)}</span>
            </div>

            {/* Зеленый бейдж: Проксированный трафик */}
            <div className="traffic-badge proxy" title={`Проксированный трафик через Mihomo/VPS\nПик: ${formatSpeed(peakProxy)}`}>
              <span className="badge-label">🟢 Прокси:</span>
              <span className="badge-arrow">↓</span>
              <span className="badge-val">{formatSpeed(currentProxy.down)}</span>
              <span className="badge-arrow">↑</span>
              <span className="badge-val">{formatSpeed(currentProxy.up)}</span>
            </div>

            {/* Суммарный трафик */}
            <div className="traffic-badge total" title="Суммарная нагрузка на канал WAN">
              <span className="badge-label">Всего:</span>
              <span className="badge-val">{formatSpeed(totalDown + totalUp)}</span>
            </div>
          </div>
        </div>
      </div>

      <div className="traffic-svg-wrapper">
        <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="traffic-svg">
          <defs>
            {/* Синий градиент (Прямой трафик) */}
            <linearGradient id="directGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#3b82f6" stopOpacity="0.32" />
              <stop offset="100%" stopColor="#3b82f6" stopOpacity="0.0" />
            </linearGradient>
            {/* Зеленый градиент (Проксированный трафик) */}
            <linearGradient id="proxyGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#10b981" stopOpacity="0.32" />
              <stop offset="100%" stopColor="#10b981" stopOpacity="0.0" />
            </linearGradient>
          </defs>

          {/* Сетка графика */}
          <line x1="0" y1={height * 0.25} x2={width} y2={height * 0.25} stroke="rgba(255,255,255,0.06)" strokeDasharray="3 3" />
          <line x1="0" y1={height * 0.5} x2={width} y2={height * 0.5} stroke="rgba(255,255,255,0.06)" strokeDasharray="3 3" />
          <line x1="0" y1={height * 0.75} x2={width} y2={height * 0.75} stroke="rgba(255,255,255,0.06)" strokeDasharray="3 3" />

          {/* Метка шкалы пика */}
          <text x={8} y={16} fill="rgba(255,255,255,0.35)" fontSize="10" fontFamily="Consolas, monospace">
            {formatSpeed(maxVal)}
          </text>
          <text x={8} y={height * 0.5 + 4} fill="rgba(255,255,255,0.22)" fontSize="9" fontFamily="Consolas, monospace">
            {formatSpeed(maxVal * 0.5)}
          </text>

          {/* Заливки областей */}
          <path d={areaD(directPoints)} fill="url(#directGrad)" />
          <path d={areaD(proxyPoints)} fill="url(#proxyGrad)" />

          {/* Синяя линия (Прямой трафик) */}
          <path
            d={lineD(directPoints)}
            fill="none"
            stroke="#3b82f6"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />

          {/* Зеленая линия (Проксированный трафик) */}
          <path
            d={lineD(proxyPoints)}
            fill="none"
            stroke="#10b981"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </div>
    </div>
  )
}
