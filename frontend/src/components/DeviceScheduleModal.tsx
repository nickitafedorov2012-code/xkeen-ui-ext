import { useEffect, useState } from 'react'
import { apiGet, apiPost } from '../api'

export interface DeviceSchedule {
  id: string
  ip: string
  enabled: boolean
  time_start: string // "23:00"
  time_end: string // "07:00"
  days: number[] // 1..=7
  action: 'block' | 'direct' | 'proxy'
  target_server?: string
}

interface DeviceScheduleModalProps {
  isOpen: boolean
  onClose: () => void
  deviceIp: string
  deviceName: string
  notify: (msg: string, error?: boolean) => void
  availableServers: string[]
}

const DAY_NAMES = [
  { id: 1, label: 'Пн' },
  { id: 2, label: 'Вт' },
  { id: 3, label: 'Ср' },
  { id: 4, label: 'Чт' },
  { id: 5, label: 'Пт' },
  { id: 6, label: 'Сб' },
  { id: 7, label: 'Вс' },
]

export default function DeviceScheduleModal({
  isOpen,
  onClose,
  deviceIp,
  deviceName,
  notify,
  availableServers,
}: DeviceScheduleModalProps) {
  const [allSchedules, setAllSchedules] = useState<DeviceSchedule[]>([])
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)

  // Новое / редактируемое расписание
  const [startTime, setStartTime] = useState('23:00')
  const [endTime, setEndTime] = useState('07:00')
  const [selectedDays, setSelectedDays] = useState<number[]>([1, 2, 3, 4, 5, 6, 7])
  const [action, setAction] = useState<'block' | 'direct' | 'proxy'>('block')
  const [targetServer, setTargetServer] = useState('')

  useEffect(() => {
    if (!isOpen) return
    const load = async () => {
      setLoading(true)
      try {
        const res = await apiGet<{ schedules: DeviceSchedule[] }>('schedules')
        setAllSchedules(res.schedules || [])
      } catch (e: any) {
        notify('Ошибка загрузки расписаний: ' + e.message, true)
      } finally {
        setLoading(false)
      }
    }
    load()
  }, [isOpen, notify])

  if (!isOpen) return null

  const deviceSchedules = allSchedules.filter((s) => s.ip === deviceIp)

  const toggleDay = (dayId: number) => {
    setSelectedDays((prev) =>
      prev.includes(dayId) ? prev.filter((d) => d !== dayId) : [...prev, dayId].sort()
    )
  }

  const handleAddSchedule = async (e: React.FormEvent) => {
    e.preventDefault()
    if (selectedDays.length === 0) {
      notify('Выберите хотя бы один день недели', true)
      return
    }

    const newSched: DeviceSchedule = {
      id: 'sched_' + Date.now(),
      ip: deviceIp,
      enabled: true,
      time_start: startTime,
      time_end: endTime,
      days: selectedDays,
      action,
      target_server: action === 'proxy' ? targetServer : undefined,
    }

    const updated = [...allSchedules, newSched]
    setSaving(true)
    try {
      await apiPost('schedules', { schedules: updated })
      setAllSchedules(updated)
      notify('Расписание добавлено')
    } catch (e: any) {
      notify('Ошибка сохранения: ' + e.message, true)
    } finally {
      setSaving(false)
    }
  }

  const handleDeleteSchedule = async (id: string) => {
    const updated = allSchedules.filter((s) => s.id !== id)
    setSaving(true)
    try {
      await apiPost('schedules', { schedules: updated })
      setAllSchedules(updated)
      notify('Расписание удалено')
    } catch (e: any) {
      notify('Ошибка: ' + e.message, true)
    } finally {
      setSaving(false)
    }
  }

  const handleToggleSchedule = async (id: string) => {
    const updated = allSchedules.map((s) => (s.id === id ? { ...s, enabled: !s.enabled } : s))
    try {
      await apiPost('schedules', { schedules: updated })
      setAllSchedules(updated)
    } catch (e: any) {
      notify('Ошибка: ' + e.message, true)
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-card schedule-modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <div>
            <h3>⏰ Расписание для: {deviceName || deviceIp}</h3>
            <p className="muted">
              Автоматическая блокировка или переключение режима устройства по часам и дням недели (например, блокировка PlayStation после 23:00).
            </p>
          </div>
          <button className="btn-close" onClick={onClose}>✕</button>
        </div>

        {/* Список текущих расписаний */}
        <div className="schedules-current-list">
          <h4>Текущие правила ({deviceSchedules.length})</h4>
          {loading ? (
            <div className="muted">Загрузка…</div>
          ) : deviceSchedules.length === 0 ? (
            <div className="muted">Для этого устройства нет активных правил расписания.</div>
          ) : (
            <div className="schedule-cards">
              {deviceSchedules.map((s) => (
                <div key={s.id} className={`schedule-item-card ${s.enabled ? 'active' : 'disabled'}`}>
                  <div className="schedule-item-header">
                    <span className="schedule-time">
                      🕒 {s.time_start} — {s.time_end}
                    </span>
                    <span className={`schedule-action-badge ${s.action}`}>
                      {s.action === 'block' ? '🚫 Блокировка' : s.action === 'direct' ? '⚡ DIRECT' : `🛰 ${s.target_server || 'Прокси'}`}
                    </span>
                    <button
                      className="btn btn-xs btn-secondary"
                      onClick={() => handleToggleSchedule(s.id)}
                    >
                      {s.enabled ? 'Включено' : 'Выключено'}
                    </button>
                    <button
                      className="btn-icon-danger"
                      onClick={() => handleDeleteSchedule(s.id)}
                      title="Удалить"
                    >
                      ✕
                    </button>
                  </div>
                  <div className="schedule-days-row">
                    Дни: {s.days.map((d) => DAY_NAMES.find((n) => n.id === d)?.label).join(', ')}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Форма добавления нового расписания */}
        <form onSubmit={handleAddSchedule} className="schedule-add-form card">
          <h4>➕ Добавить интервал</h4>

          <div className="form-row">
            <div className="form-col">
              <label>Время начала:</label>
              <input
                type="time"
                className="input-text"
                value={startTime}
                onChange={(e) => setStartTime(e.target.value)}
                required
              />
            </div>
            <div className="form-col">
              <label>Время окончания:</label>
              <input
                type="time"
                className="input-text"
                value={endTime}
                onChange={(e) => setEndTime(e.target.value)}
                required
              />
            </div>
          </div>

          <div className="form-days">
            <label>Дни недели:</label>
            <div className="days-pills">
              {DAY_NAMES.map((d) => (
                <button
                  key={d.id}
                  type="button"
                  className={`day-pill ${selectedDays.includes(d.id) ? 'active' : ''}`}
                  onClick={() => toggleDay(d.id)}
                >
                  {d.label}
                </button>
              ))}
            </div>
          </div>

          <div className="form-row">
            <div className="form-col">
              <label>Действие в интервале:</label>
              <select
                className="editor-select"
                value={action}
                onChange={(e: any) => setAction(e.target.value)}
              >
                <option value="block">🚫 Блокировать интернет (REJECT)</option>
                <option value="direct">⚡ Прямое соединение (DIRECT)</option>
                <option value="proxy">🛰 Направить на сервер (Proxy)</option>
              </select>
            </div>

            {action === 'proxy' && (
              <div className="form-col">
                <label>Сервер:</label>
                <select
                  className="editor-select"
                  value={targetServer}
                  onChange={(e) => setTargetServer(e.target.value)}
                  required
                >
                  <option value="">-- Выберите сервер --</option>
                  {availableServers.map((s) => (
                    <option key={s} value={s}>{s}</option>
                  ))}
                </select>
              </div>
            )}
          </div>

          <div className="modal-actions">
            <button type="submit" className="btn btn-primary" disabled={saving}>
              {saving ? 'Сохранение…' : 'Добавить расписание'}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
