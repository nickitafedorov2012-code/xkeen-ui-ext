import { useEffect, useState, useRef } from 'react'
import { apiGet, apiPost } from '../api'

interface LogsViewerProps {
  notify: (msg: string, error?: boolean) => void
}

export default function LogsViewer({ notify }: LogsViewerProps) {
  const [source, setSource] = useState<'daemon' | 'mihomo'>('daemon')
  const [lines, setLines] = useState<string[]>([])
  const [loading, setLoading] = useState<boolean>(true)
  const [paused, setPaused] = useState<boolean>(false)
  const [filterLevel, setFilterLevel] = useState<'all' | 'error' | 'warn' | 'info'>('all')
  const [filterQuery, setFilterQuery] = useState<string>('')
  const terminalRef = useRef<HTMLDivElement>(null)

  // Загрузка логов демона или Mihomo
  useEffect(() => {
    let ws: WebSocket | null = null
    let active = true

    const fetchLogs = async () => {
      setLoading(true)
      try {
        if (source === 'daemon') {
          const res = await apiGet<{ text: string }>('logs?lines=300')
          if (active) {
            setLines(res.text ? res.text.split('\n') : [])
          }

          // Подключаем WebSocket для live-лога
          const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
          const wsUrl = `${protocol}//${window.location.host}/api/logs/ws?lines=0`
          ws = new WebSocket(wsUrl)
          ws.onmessage = (e) => {
            if (!paused && active) {
              setLines((prev) => [...prev.slice(-500), e.data])
            }
          }
        } else {
          const res = await apiGet<{ text: string }>('logs/mihomo?lines=300')
          if (active) {
            setLines(res.text ? res.text.split('\n') : [])
          }
        }
      } catch (err: any) {
        if (active) notify('Ошибка загрузки логов: ' + err.message, true)
      } finally {
        if (active) setLoading(false)
      }
    }

    fetchLogs()

    // Периодический опрос для Mihomo логов
    const interval = source === 'mihomo' ? setInterval(fetchLogs, 3000) : null

    return () => {
      active = false
      if (ws) ws.close()
      if (interval) clearInterval(interval)
    }
  }, [source, paused, notify])

  // Автопрокрутка вниз при новых строках (если не на паузе)
  useEffect(() => {
    if (!paused && terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight
    }
  }, [lines, paused])

  const handleClear = async () => {
    if (source === 'daemon') {
      try {
        await apiPost('logs/clear', {})
        setLines([])
        notify('Журнал очищен')
      } catch (err: any) {
        notify('Ошибка очистки лога: ' + err.message, true)
      }
    } else {
      setLines([])
    }
  }

  // Фильтрация строк
  const filteredLines = lines.filter((l) => {
    if (!l.trim()) return false
    if (filterQuery && !l.toLowerCase().includes(filterQuery.toLowerCase())) {
      return false
    }
    if (filterLevel === 'error') {
      return l.toLowerCase().includes('error') || l.toLowerCase().includes('err') || l.includes('❌') || l.includes('🔴')
    }
    if (filterLevel === 'warn') {
      return l.toLowerCase().includes('warn') || l.includes('⚠️') || l.includes('🟡')
    }
    if (filterLevel === 'info') {
      return l.toLowerCase().includes('info') || l.includes('✓') || l.includes('🟢')
    }
    return true
  })

  return (
    <div className="logs-viewer-container">
      {/* Панель управления логами */}
      <div className="logs-toolbar">
        <div className="logs-source-tabs">
          <button
            className={`btn btn-sm ${source === 'daemon' ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => setSource('daemon')}
          >
            🖥 XKeen Route Daemon
          </button>
          <button
            className={`btn btn-sm ${source === 'mihomo' ? 'btn-primary' : 'btn-secondary'}`}
            onClick={() => setSource('mihomo')}
          >
            ⚙️ Mihomo Core (Ядро)
          </button>
        </div>

        <div className="logs-filters">
          <input
            type="text"
            className="input-text logs-search"
            placeholder="🔍 Фильтр по тексту…"
            value={filterQuery}
            onChange={(e) => setFilterQuery(e.target.value)}
          />

          <select
            className="editor-select"
            value={filterLevel}
            onChange={(e: any) => setFilterLevel(e.target.value)}
          >
            <option value="all">Все уровни</option>
            <option value="info">🟢 Info</option>
            <option value="warn">🟡 Warn</option>
            <option value="error">🔴 Error</option>
          </select>

          <button
            className={`btn btn-sm ${paused ? 'btn-warn' : 'btn-secondary'}`}
            onClick={() => setPaused(!paused)}
            title={paused ? 'Возобновить автоскролл' : 'Приостановить поток'}
          >
            {paused ? '▶ Продолжить' : '⏸ Пауза'}
          </button>

          <a
            href="/api/logs/download"
            download="xkeen-route.log"
            className="btn btn-sm btn-secondary"
            title="Скачать полный лог"
          >
            📥 Скачать
          </a>

          <button className="btn btn-sm btn-secondary" onClick={handleClear} title="Очистить лог">
            🗑 Очистить
          </button>
        </div>
      </div>

      {/* Терминал вывода логов */}
      <div className="logs-terminal" ref={terminalRef}>
        {loading && lines.length === 0 ? (
          <div className="logs-loading">⏳ Загрузка журнала событий…</div>
        ) : filteredLines.length === 0 ? (
          <div className="logs-empty muted">(Логи отсутствуют или отфильтрованы)</div>
        ) : (
          filteredLines.map((line, idx) => {
            const isErr = line.toLowerCase().includes('error') || line.includes('❌') || line.includes('🔴')
            const isWarn = line.toLowerCase().includes('warn') || line.includes('⚠️') || line.includes('🟡')
            const isSuccess = line.includes('✓') || line.includes('🟢')

            let lineClass = 'log-line'
            if (isErr) lineClass += ' log-error'
            else if (isWarn) lineClass += ' log-warn'
            else if (isSuccess) lineClass += ' log-success'

            return (
              <div key={idx} className={lineClass}>
                {line}
              </div>
            )
          })
        )}
      </div>

      <div className="logs-status-bar muted">
        <span>Источник: <b>{source === 'daemon' ? 'xkeen-route (порт 1001)' : 'Mihomo Core (порт 9090)'}</b></span>
        <span>Отображается строк: <b>{filteredLines.length}</b> из {lines.length}</span>
        {paused && <span className="logs-paused-badge">⏸ Поток на паузе</span>}
      </div>
    </div>
  )
}
