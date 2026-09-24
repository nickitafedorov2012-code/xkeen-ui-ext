import { useEffect, useState, useRef } from 'react'
import { apiGet } from '../api'

interface TrafficData {
  up: number // bytes/sec
  down: number // bytes/sec
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec} B/s`
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`
  return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`
}

export default function TrafficGraph() {
  const [history, setHistory] = useState<TrafficData[]>(() =>
    Array(30).fill({ up: 0, down: 0 })
  )
  const [current, setCurrent] = useState<TrafficData>({ up: 0, down: 0 })
  const [peakDown, setPeakDown] = useState(0)
  const [peakUp, setPeakUp] = useState(0)
  const activeRef = useRef(true)

  useEffect(() => {
    activeRef.current = true

    const poll = async () => {
      try {
        const data = await apiGet<{ up: number; down: number }>('traffic/poll')
        if (!activeRef.current) return
        const up = data.up || 0
        const down = data.down || 0

        setCurrent({ up, down })
        setPeakDown((prev) => Math.max(prev, down))
        setPeakUp((prev) => Math.max(prev, up))

        setHistory((prev) => [...prev.slice(1), { up, down }])
      } catch {
        /* игнорируем одиночные ошибки опроса */
      }
    }

    poll()
    const interval = setInterval(poll, 1500)
    return () => {
      activeRef.current = false
      clearInterval(interval)
    }
  }, [])

  // Построение SVG линий и градиентов
  const maxVal = Math.max(
    ...history.map((h) => Math.max(h.down, h.up)),
    1024 * 10 // минимум 10 KB/s для масштаба
  )

  const width = 600
  const height = 120
  const step = width / (history.length - 1)

  const getPoints = (key: 'down' | 'up') => {
    return history.map((d, i) => {
      const x = i * step
      const y = height - (d[key] / maxVal) * (height - 15) - 5
      return { x, y }
    })
  }

  const downPoints = getPoints('down')
  const upPoints = getPoints('up')

  const lineD = (pts: { x: number; y: number }[]) =>
    pts.reduce((acc, p, i) => `${acc} ${i === 0 ? 'M' : 'L'} ${p.x.toFixed(1)} ${p.y.toFixed(1)}`, '')

  const areaD = (pts: { x: number; y: number }[]) => {
    const l = lineD(pts)
    return `${l} L ${width} ${height} L 0 ${height} Z`
  }

  return (
    <div className="traffic-graph-card card">
      <div className="traffic-graph-header">
        <div className="traffic-graph-title">
          <span className="traffic-icon">📈</span>
          <h3>Трафик в реальном времени</h3>
        </div>
        <div className="traffic-stats">
          <div className="traffic-badge down">
            <span className="badge-arrow">↓</span>
            <span className="badge-val">{formatSpeed(current.down)}</span>
            <span className="badge-peak" title="Пик">({formatSpeed(peakDown)})</span>
          </div>
          <div className="traffic-badge up">
            <span className="badge-arrow">↑</span>
            <span className="badge-val">{formatSpeed(current.up)}</span>
            <span className="badge-peak" title="Пик">({formatSpeed(peakUp)})</span>
          </div>
        </div>
      </div>

      <div className="traffic-svg-wrapper">
        <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="traffic-svg">
          <defs>
            <linearGradient id="downGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#00d3f2" stopOpacity="0.35" />
              <stop offset="100%" stopColor="#00d3f2" stopOpacity="0.0" />
            </linearGradient>
            <linearGradient id="upGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#10b981" stopOpacity="0.35" />
              <stop offset="100%" stopColor="#10b981" stopOpacity="0.0" />
            </linearGradient>
          </defs>

          {/* Сетка */}
          <line x1="0" y1={height * 0.25} x2={width} y2={height * 0.25} stroke="rgba(255,255,255,0.06)" strokeDasharray="3 3" />
          <line x1="0" y1={height * 0.5} x2={width} y2={height * 0.5} stroke="rgba(255,255,255,0.06)" strokeDasharray="3 3" />
          <line x1="0" y1={height * 0.75} x2={width} y2={height * 0.75} stroke="rgba(255,255,255,0.06)" strokeDasharray="3 3" />

          {/* Заливки областей */}
          <path d={areaD(downPoints)} fill="url(#downGrad)" />
          <path d={areaD(upPoints)} fill="url(#upGrad)" />

          {/* Линии графиков */}
          <path d={lineD(downPoints)} fill="none" stroke="#00d3f2" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
          <path d={lineD(upPoints)} fill="none" stroke="#10b981" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      </div>
    </div>
  )
}
