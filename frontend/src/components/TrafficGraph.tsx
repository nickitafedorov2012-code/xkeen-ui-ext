import { useEffect, useState, useRef } from 'react'
import { apiGet } from '../api'

interface SpeedPair {
  down: number
  up: number
  estimated?: boolean
  source?: string
  bytes?: number
  active?: boolean
}

interface TrafficHistoryPoint {
  direct?: SpeedPair
  proxy?: SpeedPair
  whitelist?: SpeedPair
  down?: number
  up?: number
}

interface TrafficPayload {
  direct?: SpeedPair
  proxy?: SpeedPair
  whitelist?: SpeedPair
  total?: SpeedPair
  whitelist_active?: boolean
  whitelist_bytes?: number
  direct_bytes?: number
  proxy_bytes?: number
  down?: number
  up?: number
  history?: TrafficHistoryPoint[]
}

interface TrafficHistoryItem {
  directDown: number
  directUp: number
  tunnelDown: number
  tunnelUp: number
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`
  if (bytesPerSec < 1024 * 1024 * 1024) return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`
  return `${(bytesPerSec / (1024 * 1024 * 1024)).toFixed(2)} GB/s`
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export default function TrafficGraph() {
  const [history, setHistory] = useState<TrafficHistoryItem[]>(() =>
    Array(60).fill({ directDown: 0, directUp: 0, tunnelDown: 0, tunnelUp: 0 })
  )

  const [currentDirect, setCurrentDirect] = useState<SpeedPair>({ down: 0, up: 0 })
  const [currentProxy, setCurrentProxy] = useState<SpeedPair>({ down: 0, up: 0 })
  const [currentWhitelist, setCurrentWhitelist] = useState<SpeedPair>({ down: 0, up: 0 })
  const [isWhitelistActive, setIsWhitelistActive] = useState(false)
  const [whitelistBytes, setWhitelistBytes] = useState(0)
  const [proxyBytes, setProxyBytes] = useState(0)
  const [directBytes, setDirectBytes] = useState(0)
  const [peakDirect, setPeakDirect] = useState(0)
  const [peakTunnel, setPeakTunnel] = useState(0)
  const [isDirectEstimated, setIsDirectEstimated] = useState(false)

  // Режим отображения: 'down' (входящий) | 'up' (исходящий) | 'both' (сумма)
  const [metricMode, setMetricMode] = useState<'down' | 'up' | 'both'>('down')

  const activeRef = useRef(true)
  const initialHistoryLoaded = useRef(false)

  useEffect(() => {
    activeRef.current = true

    const poll = async () => {
      if (document.hidden && initialHistoryLoaded.current) return
      try {
        const data = await apiGet<TrafficPayload>('traffic/poll')
        if (!activeRef.current || !data) return

        const wlActive = Boolean(data.whitelist_active ?? data.whitelist?.active)
        const dDown = data.direct?.down ?? 0
        const dUp = data.direct?.up ?? 0
        const pDown = data.proxy?.down ?? 0
        const pUp = data.proxy?.up ?? 0
        const wlDown = data.whitelist?.down ?? (wlActive ? (data.down ?? 0) : 0)
        const wlUp = data.whitelist?.up ?? (wlActive ? (data.up ?? 0) : 0)

        const activeTunnelDown = wlActive ? wlDown : (data.proxy?.down ?? data.down ?? 0)
        const activeTunnelUp = wlActive ? wlUp : (data.proxy?.up ?? data.up ?? 0)

        setIsWhitelistActive(wlActive)
        setCurrentWhitelist({ down: wlDown, up: wlUp })
        setCurrentProxy({ down: pDown, up: pUp })
        setCurrentDirect({ down: dDown, up: dUp, estimated: data.direct?.estimated, source: data.direct?.source })
        setIsDirectEstimated(Boolean(data.direct?.estimated))

        if (typeof data.whitelist_bytes === 'number') {
          setWhitelistBytes(data.whitelist_bytes)
        }
        if (typeof data.proxy_bytes === 'number') {
          setProxyBytes(data.proxy_bytes)
        }
        if (typeof data.direct_bytes === 'number') {
          setDirectBytes(data.direct_bytes)
        }

        setPeakDirect((prev) => Math.max(prev, dDown + dUp))
        setPeakTunnel((prev) => Math.max(prev, activeTunnelDown + activeTunnelUp))

        if (!initialHistoryLoaded.current && Array.isArray(data.history) && data.history.length > 0) {
          const mapped: TrafficHistoryItem[] = data.history.map((h) => {
            const hWl = Boolean(data.whitelist_active ?? h.whitelist?.active)
            const tDown = hWl ? (h.whitelist?.down ?? h.down ?? 0) : (h.proxy?.down ?? h.down ?? 0)
            const tUp = hWl ? (h.whitelist?.up ?? h.up ?? 0) : (h.proxy?.up ?? h.up ?? 0)
            return {
              directDown: h.direct?.down ?? 0,
              directUp: h.direct?.up ?? 0,
              tunnelDown: tDown,
              tunnelUp: tUp,
            }
          })

          let maxD = dDown + dUp
          let maxT = activeTunnelDown + activeTunnelUp
          for (const item of mapped) {
            maxD = Math.max(maxD, item.directDown + item.directUp)
            maxT = Math.max(maxT, item.tunnelDown + item.tunnelUp)
          }
          setPeakDirect((prev) => Math.max(prev, maxD))
          setPeakTunnel((prev) => Math.max(prev, maxT))

          const padded = Array(Math.max(0, 60 - mapped.length))
            .fill({ directDown: 0, directUp: 0, tunnelDown: 0, tunnelUp: 0 })
            .concat(mapped)
          setHistory(padded.slice(-60))
          initialHistoryLoaded.current = true
        } else {
          setHistory((prev) => [
            ...prev.slice(1),
            {
              directDown: dDown,
              directUp: dUp,
              tunnelDown: activeTunnelDown,
              tunnelUp: activeTunnelUp,
            },
          ])
        }
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
  const getValue = (item: TrafficHistoryItem, type: 'direct' | 'tunnel') => {
    if (type === 'direct') {
      if (metricMode === 'down') return item.directDown
      if (metricMode === 'up') return item.directUp
      return item.directDown + item.directUp
    } else {
      if (metricMode === 'down') return item.tunnelDown
      if (metricMode === 'up') return item.tunnelUp
      return item.tunnelDown + item.tunnelUp
    }
  }

  // Стабильное квантование шкалы скорости без скачков и дрожания
  const rawMax = Math.max(
    ...history.map((h) => Math.max(getValue(h, 'direct'), getValue(h, 'tunnel'))),
    1024 * 32
  )

  const getQuantizedMax = (val: number): number => {
    const tiers = [
      32 * 1024,
      64 * 1024,
      128 * 1024,
      256 * 1024,
      512 * 1024,
      1024 * 1024,
      2 * 1024 * 1024,
      5 * 1024 * 1024,
      10 * 1024 * 1024,
      25 * 1024 * 1024,
      50 * 1024 * 1024,
      100 * 1024 * 1024,
      250 * 1024 * 1024,
      500 * 1024 * 1024,
      1024 * 1024 * 1024,
    ]
    for (const t of tiers) {
      if (val <= t) return t
    }
    return Math.ceil(val / (100 * 1024 * 1024)) * 100 * 1024 * 1024
  }

  const maxVal = getQuantizedMax(rawMax)

  const width = 640
  const height = 130
  const step = width / (history.length - 1)

  const getPoints = (type: 'direct' | 'tunnel') => {
    return history.map((d, i) => {
      const x = i * step
      const val = getValue(d, type)
      const y = height - (val / maxVal) * (height - 18) - 6
      return { x, y }
    })
  }

  const directPoints = getPoints('direct')
  const tunnelPoints = getPoints('tunnel')

  const lineD = (pts: { x: number; y: number }[]) =>
    pts.reduce((acc, p, i) => `${acc} ${i === 0 ? 'M' : 'L'} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`, '')

  const areaD = (pts: { x: number; y: number }[]) => {
    const l = lineD(pts)
    return `${l} L ${width} ${height} L 0 ${height} Z`
  }

  const activeTunnelDown = isWhitelistActive ? currentWhitelist.down : currentProxy.down
  const activeTunnelUp = isWhitelistActive ? currentWhitelist.up : currentProxy.up
  const totalSpeed = currentDirect.down + currentDirect.up + activeTunnelDown + activeTunnelUp

  return (
    <div className="traffic-graph-card card">
      <div className="traffic-graph-header">
        <div className="traffic-graph-title">
          <span className="traffic-icon">📈</span>
          <div>
            <h3 style={{ margin: 0, fontSize: 15 }}>Трафик в реальном времени</h3>
            {/* Строго два канала: 🟢 Прямой трафик (всегда) + активный туннель (БС фиолетовый / VPS синий) */}
            <div className="traffic-legend-chips">
              <span className="legend-chip direct">
                <span className="legend-dot green" /> Прямой трафик {isDirectEstimated && <span style={{ opacity: 0.75, fontSize: '0.85em' }}>(оценка)</span>}
              </span>
              {isWhitelistActive ? (
                <span className="legend-chip whitelist" style={{ color: '#c084fc' }}>
                  <span className="legend-dot purple" style={{ background: '#a855f7' }} /> Туннель белых списков
                </span>
              ) : (
                <span className="legend-chip proxy">
                  <span className="legend-dot blue" /> Обычные серверы (VPS)
                </span>
              )}
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
            {/* Канал 1: 🟢 Прямой трафик — ВСЕГДА отображается */}
            <div
              className="traffic-badge direct"
              title={`Прямой трафик роутера (мимо прокси${isDirectEstimated ? ', оценочный расчёт' : ''})\nНакоплено: ${formatBytes(directBytes)}\nПик: ${formatSpeed(peakDirect)}`}
            >
              <span className="badge-label">🟢 Прямой{isDirectEstimated ? ' (оценка)' : ''}:</span>
              <span className="badge-arrow">↓</span>
              <span className="badge-val">{formatSpeed(currentDirect.down)}</span>
              <span className="badge-arrow">↑</span>
              <span className="badge-val">{formatSpeed(currentDirect.up)}</span>
            </div>

            {/* Канал 2: Проксированный туннель — динамически БС или VPS (никогда не оба сразу) */}
            {isWhitelistActive ? (
              <div
                className="traffic-badge whitelist"
                style={{ background: 'rgba(168, 85, 247, 0.12)', borderColor: 'rgba(168, 85, 247, 0.4)' }}
                title={`Туннель белых списков (ноды [БС] с лимитом)\nНакоплено: ${formatBytes(whitelistBytes)}\nПик: ${formatSpeed(peakTunnel)}`}
              >
                <span className="badge-label" style={{ color: '#c084fc', fontWeight: 600 }}>🟣 БС:</span>
                <span className="badge-arrow" style={{ color: '#c084fc' }}>↓</span>
                <span className="badge-val">{formatSpeed(currentWhitelist.down)}</span>
                <span className="badge-arrow" style={{ color: '#c084fc' }}>↑</span>
                <span className="badge-val">{formatSpeed(currentWhitelist.up)}</span>
              </div>
            ) : null}

            {!isWhitelistActive && (
              <div
                className="traffic-badge proxy"
                title={`Трафик через серверы VPS (безлимитные)\nНакоплено: ${formatBytes(proxyBytes)}\nПик: ${formatSpeed(peakTunnel)}`}
              >
                <span className="badge-label">🔵 VPS:</span>
                <span className="badge-arrow">↓</span>
                <span className="badge-val">{formatSpeed(currentProxy.down)}</span>
                <span className="badge-arrow">↑</span>
                <span className="badge-val">{formatSpeed(currentProxy.up)}</span>
              </div>
            )}

            {/* Суммарный трафик: честная сумма прямого и активного туннеля */}
            <div className="traffic-badge total" title="Суммарная нагрузка на канал WAN (входящий + исходящий трафик)">
              <span className="badge-label">Всего:</span>
              <span className="badge-val">{formatSpeed(totalSpeed)}</span>
            </div>
          </div>
        </div>
      </div>

      <div className="traffic-svg-wrapper">
        <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="traffic-svg">
          <defs>
            {/* Зелёный градиент прямого трафика (Канал 1) */}
            <linearGradient id="directGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#22c55e" stopOpacity="0.25" />
              <stop offset="100%" stopColor="#22c55e" stopOpacity="0.0" />
            </linearGradient>
            {/* Градиент активного туннеля (Канал 2: фиолетовый БС / синий VPS) */}
            <linearGradient id="tunnelGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={isWhitelistActive ? "#a855f7" : "#38bdf8"} stopOpacity="0.32" />
              <stop offset="100%" stopColor={isWhitelistActive ? "#a855f7" : "#38bdf8"} stopOpacity="0.0" />
            </linearGradient>
          </defs>

          {/* Сетка графика */}
          <line x1="0" y1={height * 0.25} x2={width} y2={height * 0.25} className="traffic-grid-line" strokeDasharray="3 3" />
          <line x1="0" y1={height * 0.5} x2={width} y2={height * 0.5} className="traffic-grid-line" strokeDasharray="3 3" />
          <line x1="0" y1={height * 0.75} x2={width} y2={height * 0.75} className="traffic-grid-line" strokeDasharray="3 3" />

          {/* Заливки областей */}
          <path d={areaD(directPoints)} fill="url(#directGrad)" />
          <path d={areaD(tunnelPoints)} fill="url(#tunnelGrad)" />

          {/* Зеленая линия прямого трафика (Канал 1: Direct) */}
          <path
            d={lineD(directPoints)}
            fill="none"
            stroke="#22c55e"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />

          {/* Линия активного туннеля (Канал 2: БС фиолетовый / VPS синий) */}
          <path
            d={lineD(tunnelPoints)}
            fill="none"
            stroke={isWhitelistActive ? "#a855f7" : "#38bdf8"}
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>

        {/* Бейджи шкалы скорости (HTML оверлей — без горизонтального растяжения) */}
        <div className="traffic-scale-badge top">
          <span className="traffic-scale-text">{formatSpeed(maxVal)}</span>
        </div>
        <div className="traffic-scale-badge mid">
          <span className="traffic-scale-text-dim">{formatSpeed(maxVal * 0.5)}</span>
        </div>
      </div>
    </div>
  )
}


