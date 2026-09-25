import { useState, useEffect, useCallback, useMemo, useRef } from 'react'
import { apiGet, apiPost } from '../api'
import type { TaskManagerSnapshot, ProcessInfo, ProcessCategory } from '../types'

interface TaskManagerProps {
  notify: (msg: string, isError?: boolean) => void
}

type SortField = 'pid' | 'cpu_percent' | 'mem_percent' | 'mem_rss_kb' | 'threads' | 'name' | 'user'
type SortOrder = 'asc' | 'desc'

export default function TaskManager({ notify }: TaskManagerProps) {
  const [snapshot, setSnapshot] = useState<TaskManagerSnapshot | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Настройки автообновления
  const [autoRefreshInterval, setAutoRefreshInterval] = useState<number>(3) // 3 сек по умолчанию
  const [isPaused, setIsPaused] = useState(false)

  // Фильтры и поиск
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedCategory, setSelectedCategory] = useState<ProcessCategory | 'all'>('all')
  const [onlyActive, setOnlyActive] = useState(false)

  // Сортировка
  const [sortField, setSortField] = useState<SortField>('cpu_percent')
  const [sortOrder, setSortOrder] = useState<SortOrder>('desc')

  // Модалка подтверждения завершения процесса
  const [confirmProcess, setConfirmProcess] = useState<ProcessInfo | null>(null)
  const [killSignal, setKillSignal] = useState<'TERM' | 'KILL'>('TERM')
  const [killBusy, setKillBusy] = useState(false)

  // Развернутый вид
  const [isExpanded, setIsExpanded] = useState(false)

  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const fetchProcesses = useCallback(async (isManual = false) => {
    if (isManual) setLoading(true)
    try {
      const data = await apiGet<TaskManagerSnapshot>('system/processes')
      setSnapshot(data)
      setError(null)
    } catch (e: any) {
      setError(e?.message || 'Ошибка загрузки данных диспетчера задач')
    } finally {
      if (isManual) setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchProcesses(true)
  }, [fetchProcesses])

  // Интервал автообновления
  useEffect(() => {
    if (isPaused || autoRefreshInterval <= 0) {
      if (timerRef.current) clearInterval(timerRef.current)
      return
    }

    timerRef.current = setInterval(() => {
      fetchProcesses(false)
    }, autoRefreshInterval * 1000)

    return () => {
      if (timerRef.current) clearInterval(timerRef.current)
    }
  }, [autoRefreshInterval, isPaused, fetchProcesses])

  // Обработчик завершения процесса
  const handleKill = async () => {
    if (!confirmProcess) return
    setKillBusy(true)
    try {
      const res = await apiPost<{ pid: number; message: string }>('system/processes/kill', {
        pid: confirmProcess.pid,
        signal: killSignal,
      })
      notify(res.message || `Сигнал ${killSignal} успешно отправлен процессу PID ${confirmProcess.pid}`)
      setConfirmProcess(null)
      // Сразу обновляем список
      setTimeout(() => fetchProcesses(false), 500)
    } catch (e: any) {
      notify(`Ошибка завершения процесса: ${e?.message || e}`, true)
    } finally {
      setKillBusy(false)
    }
  }

  // Сортировка таблицы
  const handleSort = (field: SortField) => {
    if (sortField === field) {
      setSortOrder(sortOrder === 'asc' ? 'desc' : 'asc')
    } else {
      setSortField(field)
      setSortOrder('desc')
    }
  }

  // Фильтрация процессов
  const filteredProcesses = useMemo(() => {
    if (!snapshot?.processes) return []

    return snapshot.processes
      .filter((p) => {
        // Фильтр по категории
        if (selectedCategory !== 'all' && p.category !== selectedCategory) {
          return false
        }
        // Фильтр "Только активные"
        if (onlyActive && p.cpu_percent === 0 && p.mem_percent < 0.5) {
          return false
        }
        // Поиск по подстроке
        if (searchQuery.trim()) {
          const q = searchQuery.toLowerCase().trim()
          const matchPid = p.pid.toString().includes(q)
          const matchName = p.name.toLowerCase().includes(q)
          const matchCmd = p.cmdline.toLowerCase().includes(q)
          const matchUser = p.user.toLowerCase().includes(q)
          if (!matchPid && !matchName && !matchCmd && !matchUser) {
            return false
          }
        }
        return true
      })
      .sort((a, b) => {
        let valA: any = a[sortField]
        let valB: any = b[sortField]

        if (typeof valA === 'string') {
          valA = valA.toLowerCase()
          valB = (valB as string).toLowerCase()
        }

        if (valA < valB) return sortOrder === 'asc' ? -1 : 1
        if (valA > valB) return sortOrder === 'asc' ? 1 : -1
        return 0
      })
  }, [snapshot, selectedCategory, onlyActive, searchQuery, sortField, sortOrder])

  // Категории для подсчета количеств
  const categoryCounts = useMemo(() => {
    if (!snapshot?.processes) return { all: 0, xkeen: 0, zapret: 0, keenetic: 0, system: 0 }
    const counts = { all: snapshot.processes.length, xkeen: 0, zapret: 0, keenetic: 0, system: 0 }
    for (const p of snapshot.processes) {
      if (counts[p.category] !== undefined) {
        counts[p.category]++
      } else {
        counts.system++
      }
    }
    return counts
  }, [snapshot])

  // Форматирование RSS памяти
  const formatRss = (kb: number) => {
    if (kb >= 1024 * 1024) {
      return `${(kb / (1024 * 1024)).toFixed(1)} ГБ`
    }
    if (kb >= 1024) {
      return `${(kb / 1024).toFixed(1)} МБ`
    }
    return `${kb} КБ`
  }

  // Форматирование аптайма
  const formatUptime = (sec: number) => {
    const days = Math.floor(sec / 86400)
    const hours = Math.floor((sec % 86400) / 3600)
    const minutes = Math.floor((sec % 3600) / 60)
    const parts = []
    if (days > 0) parts.push(`${days} дн.`)
    if (hours > 0 || days > 0) parts.push(`${hours} ч.`)
    parts.push(`${minutes} мин.`)
    return parts.join(' ')
  }

  // Цвет индикатора CPU / RAM
  const getMeterColor = (pct: number) => {
    if (pct >= 85) return '#ef4444' // красный
    if (pct >= 60) return '#f59e0b' // оранжевый
    if (pct >= 30) return '#00d3f2' // циановый
    return '#22c55e' // зеленый
  }

  const res = snapshot?.resources

  return (
    <div className={`task-manager-container ${isExpanded ? 'task-manager-expanded' : ''}`} style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      {/* ШАПКА ДИСПЕТЧЕРА ЗАДАЧ */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 12 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <h2 style={{ margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
            <span>📊</span> Диспетчер задач
            <span style={{ fontSize: 12, padding: '2px 8px', borderRadius: 4, background: 'rgba(0, 211, 242, 0.15)', color: '#00d3f2', border: '1px solid rgba(0, 211, 242, 0.3)', fontFamily: 'monospace' }}>
              htop-shell
            </span>
          </h2>
          {loading && <span className="muted small">⏳ Загрузка…</span>}
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
          {/* Переключатель интервала */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 13 }}>
            <span className="muted">Автообновление:</span>
            <select
              className="select"
              style={{ width: 'auto', padding: '4px 8px', fontSize: 13 }}
              value={isPaused ? 0 : autoRefreshInterval}
              onChange={(e) => {
                const val = Number(e.target.value)
                if (val === 0) {
                  setIsPaused(true)
                } else {
                  setIsPaused(false)
                  setAutoRefreshInterval(val)
                }
              }}
            >
              <option value={2}>2 сек</option>
              <option value={3}>3 сек</option>
              <option value={5}>5 сек</option>
              <option value={10}>10 сек</option>
              <option value={0}>⏸ Пауза</option>
            </select>
          </div>

          <button
            type="button"
            className="btn sm"
            onClick={() => fetchProcesses(true)}
            disabled={loading}
            title="Обновить список процессов вручную"
          >
            🔄 Обновить
          </button>

          <button
            type="button"
            className="btn sm ghost"
            onClick={() => setIsExpanded(!isExpanded)}
            title={isExpanded ? 'Свернуть в стандартный вид' : 'Развернуть на весь экран'}
          >
            {isExpanded ? '🗗 Свернуть' : '🗖 Во весь экран'}
          </button>
        </div>
      </div>

      {error && (
        <div className="card" style={{ borderLeft: '4px solid #ef4444', background: 'rgba(239, 68, 68, 0.1)', padding: 12 }}>
          <div style={{ color: '#ef4444', fontWeight: 600 }}>Ошибка получения данных</div>
          <div className="small muted">{error}</div>
        </div>
      )}

      {/* HTOP HUD GAUGE PANEL (ШКАЛЫ ПРОЦЕССОРА И ПАМЯТИ) */}
      {res && (
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(360px, 1fr))',
            gap: 12,
            background: 'rgba(13, 17, 23, 0.7)',
            border: '1px solid var(--border)',
            borderRadius: 8,
            padding: 14,
            fontFamily: 'monospace',
            fontSize: 13,
          }}
        >
          {/* ЛЕВАЯ КОЛОНКА HUD: CPU TOTAL И ЯДРА */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {/* Общий CPU */}
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                <span style={{ color: '#00d3f2', fontWeight: 600 }}>CPU Общий</span>
                <span style={{ color: getMeterColor(res.cpu_total_percent), fontWeight: 700 }}>
                  {res.cpu_total_percent.toFixed(1)}%
                </span>
              </div>
              <div style={{ height: 10, background: 'rgba(255, 255, 255, 0.08)', borderRadius: 4, overflow: 'hidden' }}>
                <div
                  style={{
                    height: '100%',
                    width: `${Math.min(res.cpu_total_percent, 100)}%`,
                    background: getMeterColor(res.cpu_total_percent),
                    transition: 'width 0.3s ease, background 0.3s ease',
                  }}
                />
              </div>
            </div>

            {/* Ядра процессора роутера */}
            {res.cpu_cores && res.cpu_cores.length > 0 && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6, marginTop: 4 }}>
                {res.cpu_cores.map((core) => (
                  <div key={core.core_id}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, marginBottom: 2 }}>
                      <span className="muted">Ядро {core.core_id}</span>
                      <span style={{ color: getMeterColor(core.usage_percent) }}>{core.usage_percent.toFixed(1)}%</span>
                    </div>
                    <div style={{ height: 6, background: 'rgba(255, 255, 255, 0.06)', borderRadius: 3, overflow: 'hidden' }}>
                      <div
                        style={{
                          height: '100%',
                          width: `${Math.min(core.usage_percent, 100)}%`,
                          background: getMeterColor(core.usage_percent),
                          transition: 'width 0.3s ease',
                        }}
                      />
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          {/* ПРАВАЯ КОЛОНКА HUD: ОЗУ, ПОДКАЧКА, ЗАДАЧИ, UPTIME */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {/* ОЗУ (RAM) */}
            <div>
              <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
                <span style={{ color: '#22c55e', fontWeight: 600 }}>MEM Память</span>
                <span>
                  <b style={{ color: getMeterColor(res.memory_percent) }}>
                    {(res.memory_used_kb / 1024).toFixed(0)} МБ
                  </b>
                  <span className="muted"> / {(res.memory_total_kb / 1024).toFixed(0)} МБ ({res.memory_percent.toFixed(1)}%)</span>
                </span>
              </div>
              <div style={{ height: 10, background: 'rgba(255, 255, 255, 0.08)', borderRadius: 4, overflow: 'hidden' }}>
                <div
                  style={{
                    height: '100%',
                    width: `${Math.min(res.memory_percent, 100)}%`,
                    background: getMeterColor(res.memory_percent),
                    transition: 'width 0.3s ease',
                  }}
                />
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--muted)', marginTop: 2 }}>
                <span>Кэш: {(res.memory_cached_kb / 1024).toFixed(0)} МБ</span>
                <span>Буферы: {(res.memory_buffers_kb / 1024).toFixed(0)} МБ</span>
                <span>Свободно: {(res.memory_free_kb / 1024).toFixed(0)} МБ</span>
              </div>
            </div>

            {/* Сводные показатели Tasks & Load Average */}
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginTop: 4, fontSize: 12 }}>
              <div style={{ background: 'rgba(255, 255, 255, 0.03)', padding: '6px 10px', borderRadius: 4 }}>
                <span className="muted">Задачи: </span>
                <b style={{ color: 'var(--text-bright)' }}>{res.tasks_total}</b> всего,{' '}
                <span style={{ color: '#22c55e' }}>{res.tasks_running} акт.</span>
              </div>
              <div style={{ background: 'rgba(255, 255, 255, 0.03)', padding: '6px 10px', borderRadius: 4 }}>
                <span className="muted">Load avg: </span>
                <span style={{ color: '#00d3f2' }}>
                  {res.load_avg_1m.toFixed(2)}, {res.load_avg_5m.toFixed(2)}, {res.load_avg_15m.toFixed(2)}
                </span>
              </div>
            </div>

            <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, background: 'rgba(255, 255, 255, 0.03)', padding: '6px 10px', borderRadius: 4 }}>
              <span className="muted">Время работы роутера (Uptime):</span>
              <span style={{ color: 'var(--text-bright)', fontWeight: 600 }}>{formatUptime(res.uptime_seconds)}</span>
            </div>
          </div>
        </div>
      )}

      {/* ПАНЕЛЬ УПРАВЛЕНИЯ: ФИЛЬТРЫ КАТЕГОРИЙ И ПОИСК */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 10 }}>
        {/* Кнопки-таблетки категорий */}
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
          <button
            type="button"
            className={`btn sm ${selectedCategory === 'all' ? 'primary' : 'ghost'}`}
            onClick={() => setSelectedCategory('all')}
          >
            Все ({categoryCounts.all})
          </button>
          <button
            type="button"
            className={`btn sm ${selectedCategory === 'xkeen' ? 'primary' : 'ghost'}`}
            style={selectedCategory === 'xkeen' ? {} : { color: '#00d3f2', borderColor: 'rgba(0, 211, 242, 0.3)' }}
            onClick={() => setSelectedCategory('xkeen')}
            title="Процессы ядра Mihomo и панели XKeen Route"
          >
            ⚡ XKeen / Mihomo ({categoryCounts.xkeen})
          </button>
          <button
            type="button"
            className={`btn sm ${selectedCategory === 'zapret' ? 'primary' : 'ghost'}`}
            style={selectedCategory === 'zapret' ? {} : { color: '#c084fc', borderColor: 'rgba(192, 132, 252, 0.3)' }}
            onClick={() => setSelectedCategory('zapret')}
            title="Процессы DPI-обхода (nfqws / tpws)"
          >
            🛡 Zapret ({categoryCounts.zapret})
          </button>
          <button
            type="button"
            className={`btn sm ${selectedCategory === 'keenetic' ? 'primary' : 'ghost'}`}
            style={selectedCategory === 'keenetic' ? {} : { color: '#60a5fa', borderColor: 'rgba(96, 165, 250, 0.3)' }}
            onClick={() => setSelectedCategory('keenetic')}
            title="Системные службы KeeneticOS (ndm, dnsmasq, dropbear, wifi)"
          >
            🌐 KeeneticOS ({categoryCounts.keenetic})
          </button>
          <button
            type="button"
            className={`btn sm ${selectedCategory === 'system' ? 'primary' : 'ghost'}`}
            onClick={() => setSelectedCategory('system')}
          >
            Системные ({categoryCounts.system})
          </button>
        </div>

        {/* Поиск и тумблер активности */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
          <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 13, cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={onlyActive}
              onChange={(e) => setOnlyActive(e.target.checked)}
            />
            <span className="muted">Только с нагрузкой</span>
          </label>

          <div style={{ position: 'relative', width: 220 }}>
            <input
              type="text"
              className="input sm"
              placeholder="🔍 Поиск PID / имя / команда…"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              style={{ width: '100%', paddingRight: 24, fontSize: 13 }}
            />
            {searchQuery && (
              <button
                type="button"
                onClick={() => setSearchQuery('')}
                style={{
                  position: 'absolute',
                  right: 6,
                  top: '50%',
                  transform: 'translateY(-50%)',
                  background: 'none',
                  border: 'none',
                  color: 'var(--muted)',
                  cursor: 'pointer',
                  padding: 0,
                  fontSize: 12,
                }}
              >
                ✕
              </button>
            )}
          </div>
        </div>
      </div>

      {/* ТАБЛИЦА ПРОЦЕССОВ HTOP */}
      <div style={{ overflowX: 'auto', border: '1px solid var(--border)', borderRadius: 8 }}>
        <table className="table" style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13, fontFamily: 'monospace' }}>
          <thead>
            <tr style={{ background: 'rgba(255, 255, 255, 0.04)', borderBottom: '1px solid var(--border)', textAlign: 'left' }}>
              <th
                onClick={() => handleSort('pid')}
                style={{ padding: '8px 12px', cursor: 'pointer', userSelect: 'none', width: 70 }}
                title="Идентификатор процесса"
              >
                PID {sortField === 'pid' ? (sortOrder === 'asc' ? '▲' : '▼') : ''}
              </th>
              <th
                onClick={() => handleSort('user')}
                style={{ padding: '8px 10px', cursor: 'pointer', userSelect: 'none', width: 80 }}
              >
                USER {sortField === 'user' ? (sortOrder === 'asc' ? '▲' : '▼') : ''}
              </th>
              <th style={{ padding: '8px 8px', width: 45, textAlign: 'center' }} title="Состояние (R: активен, S: спит, D: ожидает диск, Z: зомби)">
                ST
              </th>
              <th
                onClick={() => handleSort('cpu_percent')}
                style={{ padding: '8px 10px', cursor: 'pointer', userSelect: 'none', width: 90, textAlign: 'right' }}
                title="Нагрузка на процессор (в процентах одного ядра)"
              >
                CPU% {sortField === 'cpu_percent' ? (sortOrder === 'asc' ? '▲' : '▼') : ''}
              </th>
              <th
                onClick={() => handleSort('mem_percent')}
                style={{ padding: '8px 10px', cursor: 'pointer', userSelect: 'none', width: 90, textAlign: 'right' }}
                title="Процент физической памяти"
              >
                MEM% {sortField === 'mem_percent' ? (sortOrder === 'asc' ? '▲' : '▼') : ''}
              </th>
              <th
                onClick={() => handleSort('mem_rss_kb')}
                style={{ padding: '8px 10px', cursor: 'pointer', userSelect: 'none', width: 95, textAlign: 'right' }}
                title="Физическая память (RSS)"
              >
                RSS {sortField === 'mem_rss_kb' ? (sortOrder === 'asc' ? '▲' : '▼') : ''}
              </th>
              <th
                onClick={() => handleSort('threads')}
                style={{ padding: '8px 8px', cursor: 'pointer', userSelect: 'none', width: 55, textAlign: 'center' }}
                title="Количество потоков"
              >
                THR {sortField === 'threads' ? (sortOrder === 'asc' ? '▲' : '▼') : ''}
              </th>
              <th
                onClick={() => handleSort('name')}
                style={{ padding: '8px 12px', cursor: 'pointer', userSelect: 'none' }}
                title="Имя и командная строка запуска"
              >
                КОМАНДА / ПРОЦЕСС {sortField === 'name' ? (sortOrder === 'asc' ? '▲' : '▼') : ''}
              </th>
              <th style={{ padding: '8px 12px', width: 140, textAlign: 'center' }}>
                ДЕЙСТВИЯ
              </th>
            </tr>
          </thead>
          <tbody>
            {filteredProcesses.length === 0 ? (
              <tr>
                <td colSpan={9} style={{ textAlign: 'center', padding: '24px 0', color: 'var(--muted)' }}>
                  {snapshot ? 'Процессы по заданным критериям не найдены' : 'Загрузка списка процессов…'}
                </td>
              </tr>
            ) : (
              filteredProcesses.map((p) => {
                const isSelected = confirmProcess?.pid === p.pid
                let stateBadgeColor = 'var(--muted)'
                if (p.state === 'R') stateBadgeColor = '#22c55e'
                else if (p.state === 'D') stateBadgeColor = '#f59e0b'
                else if (p.state === 'Z') stateBadgeColor = '#ef4444'

                return (
                  <tr
                    key={p.pid}
                    style={{
                      borderBottom: '1px solid rgba(255, 255, 255, 0.05)',
                      background: isSelected ? 'rgba(0, 211, 242, 0.1)' : 'transparent',
                    }}
                  >
                    {/* PID */}
                    <td style={{ padding: '7px 12px', fontWeight: 600, color: p.protected ? '#60a5fa' : 'var(--text-bright)' }}>
                      {p.pid}
                    </td>

                    {/* USER */}
                    <td style={{ padding: '7px 10px', color: p.user === 'root' ? 'var(--text-bright)' : 'var(--muted)' }}>
                      {p.user}
                    </td>

                    {/* STATE */}
                    <td style={{ padding: '7px 8px', textAlign: 'center' }}>
                      <span
                        style={{
                          display: 'inline-block',
                          padding: '1px 5px',
                          borderRadius: 3,
                          fontSize: 11,
                          fontWeight: 700,
                          background: `${stateBadgeColor}20`,
                          color: stateBadgeColor,
                          border: `1px solid ${stateBadgeColor}40`,
                        }}
                        title={`Состояние: ${p.state}`}
                      >
                        {p.state}
                      </span>
                    </td>

                    {/* CPU % */}
                    <td style={{ padding: '7px 10px', textAlign: 'right' }}>
                      <span style={{ color: getMeterColor(p.cpu_percent), fontWeight: p.cpu_percent > 1 ? 700 : 400 }}>
                        {p.cpu_percent > 0 ? p.cpu_percent.toFixed(1) : '0.0'}%
                      </span>
                    </td>

                    {/* MEM % */}
                    <td style={{ padding: '7px 10px', textAlign: 'right' }}>
                      <span style={{ color: getMeterColor(p.mem_percent), fontWeight: p.mem_percent > 2 ? 700 : 400 }}>
                        {p.mem_percent > 0 ? p.mem_percent.toFixed(1) : '0.0'}%
                      </span>
                    </td>

                    {/* RSS */}
                    <td style={{ padding: '7px 10px', textAlign: 'right', color: 'var(--text-bright)' }}>
                      {formatRss(p.mem_rss_kb)}
                    </td>

                    {/* THREADS */}
                    <td style={{ padding: '7px 8px', textAlign: 'center', color: 'var(--muted)' }}>
                      {p.threads}
                    </td>

                    {/* COMMAND / NAME */}
                    <td style={{ padding: '7px 12px', maxWidth: 360, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={p.cmdline}>
                      {p.category === 'xkeen' && (
                        <span style={{ display: 'inline-block', fontSize: 10, padding: '1px 5px', borderRadius: 3, background: 'rgba(0, 211, 242, 0.15)', color: '#00d3f2', border: '1px solid rgba(0, 211, 242, 0.3)', marginRight: 6 }}>
                          XKEEN
                        </span>
                      )}
                      {p.category === 'zapret' && (
                        <span style={{ display: 'inline-block', fontSize: 10, padding: '1px 5px', borderRadius: 3, background: 'rgba(192, 132, 252, 0.15)', color: '#c084fc', border: '1px solid rgba(192, 132, 252, 0.3)', marginRight: 6 }}>
                          ZAPRET
                        </span>
                      )}
                      {p.category === 'keenetic' && (
                        <span style={{ display: 'inline-block', fontSize: 10, padding: '1px 5px', borderRadius: 3, background: 'rgba(96, 165, 250, 0.15)', color: '#60a5fa', border: '1px solid rgba(96, 165, 250, 0.3)', marginRight: 6 }}>
                          KEENETIC
                        </span>
                      )}
                      <b style={{ color: 'var(--text-bright)', marginRight: 6 }}>{p.name}</b>
                      <span className="muted" style={{ fontSize: 12 }}>
                        {p.cmdline !== `[${p.name}]` && p.cmdline !== p.name ? p.cmdline : ''}
                      </span>
                    </td>

                    {/* ACTIONS */}
                    <td style={{ padding: '7px 12px', textAlign: 'center' }}>
                      {p.protected ? (
                        <span
                          style={{
                            display: 'inline-flex',
                            alignItems: 'center',
                            gap: 3,
                            fontSize: 11,
                            color: 'var(--muted)',
                            background: 'rgba(255, 255, 255, 0.04)',
                            padding: '2px 6px',
                            borderRadius: 4,
                          }}
                          title="Системный процесс защищен от завершения"
                        >
                          🛡 Защищён
                        </span>
                      ) : (
                        <div style={{ display: 'inline-flex', gap: 4 }}>
                          <button
                            type="button"
                            className="btn sm ghost"
                            style={{ padding: '2px 6px', fontSize: 11, color: '#f59e0b' }}
                            onClick={() => {
                              setConfirmProcess(p)
                              setKillSignal('TERM')
                            }}
                            title={`Завершить процесс ${p.name} сигналом SIGTERM`}
                          >
                            🛑 Term
                          </button>
                          <button
                            type="button"
                            className="btn sm ghost"
                            style={{ padding: '2px 6px', fontSize: 11, color: '#ef4444' }}
                            onClick={() => {
                              setConfirmProcess(p)
                              setKillSignal('KILL')
                            }}
                            title={`Принудительно убить процесс ${p.name} сигналом SIGKILL (-9)`}
                          >
                            ⚡ Kill
                          </button>
                        </div>
                      )}
                    </td>
                  </tr>
                )
              })
            )}
          </tbody>
        </table>
      </div>

      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', fontSize: 12, color: 'var(--muted)', flexWrap: 'wrap', gap: 8 }}>
        <span>
          Отображено процессов: <b>{filteredProcesses.length}</b> из <b>{snapshot?.processes.length || 0}</b>
        </span>
        <span>
          💡 Совет: Для сортировки нажмите на заголовок любого столбца (CPU%, MEM%, PID).
        </span>
      </div>

      {/* МОДАЛЬНОЕ ОКНО ПОДТВЕРЖДЕНИЯ ЗАВЕРШЕНИЯ ПРОЦЕССА */}
      {confirmProcess && (
        <div
          style={{
            position: 'fixed',
            inset: 0,
            background: 'rgba(0, 0, 0, 0.7)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            zIndex: 1000,
            backdropFilter: 'blur(3px)',
          }}
          onClick={() => !killBusy && setConfirmProcess(null)}
        >
          <div
            className="card"
            style={{ width: 440, maxWidth: '90%', padding: 20, border: '1px solid var(--border)' }}
            onClick={(e) => e.stopPropagation()}
          >
            <h3 style={{ margin: '0 0 12px', display: 'flex', alignItems: 'center', gap: 8, color: killSignal === 'KILL' ? '#ef4444' : '#f59e0b' }}>
              <span>{killSignal === 'KILL' ? '⚡ Принудительное завершение' : '🛑 Завершение процесса'}</span>
            </h3>

            <p style={{ margin: '0 0 12px', fontSize: 14 }}>
              Вы действительно хотите отправить сигнал <b>SIG{killSignal}</b> процессу:
            </p>

            <div
              style={{
                background: 'rgba(0, 0, 0, 0.3)',
                padding: 12,
                borderRadius: 6,
                fontFamily: 'monospace',
                fontSize: 13,
                marginBottom: 16,
                display: 'flex',
                flexDirection: 'column',
                gap: 4,
              }}
            >
              <div>
                <span className="muted">PID: </span>
                <b style={{ color: 'var(--text-bright)' }}>{confirmProcess.pid}</b>
              </div>
              <div>
                <span className="muted">Имя: </span>
                <b style={{ color: '#00d3f2' }}>{confirmProcess.name}</b>
              </div>
              <div>
                <span className="muted">Пользователь: </span>
                <span>{confirmProcess.user}</span>
              </div>
              <div style={{ wordBreak: 'break-all', marginTop: 4 }}>
                <span className="muted">Команда: </span>
                <span style={{ fontSize: 11, color: 'var(--text-bright)' }}>{confirmProcess.cmdline}</span>
              </div>
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginBottom: 16 }}>
              <span className="muted small">Тип сигнала:</span>
              <div style={{ display: 'flex', gap: 8 }}>
                <button
                  type="button"
                  className={`btn sm ${killSignal === 'TERM' ? 'primary' : 'ghost'}`}
                  onClick={() => setKillSignal('TERM')}
                  style={{ flex: 1 }}
                >
                  🛑 SIGTERM (Мягкое)
                </button>
                <button
                  type="button"
                  className={`btn sm ${killSignal === 'KILL' ? 'primary' : 'ghost'}`}
                  onClick={() => setKillSignal('KILL')}
                  style={{ flex: 1, color: killSignal === 'KILL' ? '#fff' : '#ef4444', borderColor: '#ef4444' }}
                >
                  ⚡ SIGKILL (Принудительно -9)
                </button>
              </div>
            </div>

            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
              <button
                type="button"
                className="btn ghost"
                onClick={() => setConfirmProcess(null)}
                disabled={killBusy}
              >
                Отмена
              </button>
              <button
                type="button"
                className="btn primary"
                style={{ background: killSignal === 'KILL' ? '#ef4444' : undefined }}
                onClick={handleKill}
                disabled={killBusy}
              >
                {killBusy ? 'Отправка сигнала…' : 'Подтвердить завершение'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
