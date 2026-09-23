import { useEffect, useState } from 'react'
import { apiGet, apiPost } from '../api'
import { pingClass, type DeviceDomainRule, type ServerInfo } from '../types'
import NumberInput from './NumberInput'

export interface DrModalState {
  ip: string
  name: string
  servers: string[]
  threshold: number
  autoRestore: boolean
}

interface Props {
  modal: DrModalState
  servers: ServerInfo[]
  busy: boolean
  hasExisting: boolean
  onChange: (m: DrModalState) => void
  onClose: () => void
  onSave: () => void
  serverLabel: (id: string) => string
}

/// Модалка «Сервер, резервы и персональные правила доменов»
export default function DeviceRoutingModal({
  modal,
  servers,
  busy,
  hasExisting,
  onChange,
  onClose,
  onSave,
  serverLabel,
}: Props) {
  const [domainRules, setDomainRules] = useState<DeviceDomainRule[]>([])
  const [newDomain, setNewDomain] = useState('')
  const [newTarget, setNewTarget] = useState('DIRECT')
  const [savingRules, setSavingRules] = useState(false)

  // Загрузка персональных правил для доменов устройства
  useEffect(() => {
    apiGet<{ rules: Record<string, DeviceDomainRule[]> }>('devices/domain-rules')
      .then((res) => {
        setDomainRules(res.rules[modal.ip] || [])
      })
      .catch(() => {})
  }, [modal.ip])

  const drMove = (idx: number, dir: -1 | 1) => {
    const next = [...modal.servers]
    const j = idx + dir
    if (j < 0 || j >= next.length) return
    ;[next[idx], next[j]] = [next[j], next[idx]]
    onChange({ ...modal, servers: next })
  }

  const handleAddDomainRule = (e: React.FormEvent) => {
    e.preventDefault()
    const d = newDomain.trim().toLowerCase()
    if (!d) return
    if (domainRules.some((r) => r.domain === d)) return
    setDomainRules((prev) => [...prev, { domain: d, target: newTarget }])
    setNewDomain('')
  }

  const handleRemoveDomainRule = (idx: number) => {
    setDomainRules((prev) => prev.filter((_, i) => i !== idx))
  }

  const handleSaveAll = async () => {
    setSavingRules(true)
    try {
      await apiPost('devices/domain-rules', {
        ip: modal.ip,
        rules: domainRules,
      })
    } catch {
      /* игнорируем ошибку сохранения правил доменов, чтобы сохранить основной роутинг */
    } finally {
      setSavingRules(false)
      onSave()
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" style={{ maxWidth: 640 }} onClick={(e) => e.stopPropagation()}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
          <h2 style={{ margin: 0 }}>🛡️ Маршрутизация — {modal.name}</h2>
          <button type="button" className="btn sm ghost" onClick={onClose} style={{ fontSize: 16, padding: '2px 8px' }}>
            ✕
          </button>
        </div>
        <p className="muted small" style={{ margin: '0 0 12px' }}>
          IP-адрес: <b style={{ fontFamily: 'monospace' }}>{modal.ip}</b>. Настройка цепочки отказоустойчивости (failover) и персональных доменных правил для этого устройства.
        </p>

        {/* СЕКЦИЯ 1: ЦЕПОЧКА СЕРВЕРОВ */}
        <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: 12, marginBottom: 14 }}>
          <b style={{ fontSize: 13, display: 'block', marginBottom: 6 }}>🛰 Цепочка серверов (Failover)</b>
          <p className="muted small" style={{ margin: '0 0 8px' }}>
            Первый — основной, остальные — резервы при сбое или высоком пинге.
          </p>
          <div className="modal-list" style={{ maxHeight: 180, marginBottom: 8 }}>
            {modal.servers.length === 0 && (
              <p className="muted small" style={{ margin: '4px 0' }}>Цепочка пуста — устройство использует глобальный PROXY.</p>
            )}
            {modal.servers.map((id, i) => {
              const s = servers.find((x) => x.id === id)
              return (
                <div key={id} className="check-row" style={{ display: 'flex', gap: 6, alignItems: 'center', padding: '4px 8px' }}>
                  <span className="badge">{i === 0 ? 'ОСН' : `РЕЗ${i}`}</span>
                  <span className="server-name" style={{ flex: 1 }} title={id}>{s ? s.name : id}</span>
                  {s && <span className={'ping ' + pingClass(s.ping_ms)}>{s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}</span>}
                  <button type="button" className="btn sm ghost" disabled={i === 0} onClick={() => drMove(i, -1)}>↑</button>
                  <button type="button" className="btn sm ghost" disabled={i === modal.servers.length - 1} onClick={() => drMove(i, 1)}>↓</button>
                  <button
                    type="button"
                    className="btn sm ghost"
                    onClick={() => onChange({ ...modal, servers: modal.servers.filter((x) => x !== id) })}
                  >
                    ✕
                  </button>
                </div>
              )
            })}
          </div>

          <select
            className="select"
            value=""
            onChange={(e) => {
              if (!e.target.value) return
              if (!modal.servers.includes(e.target.value)) {
                onChange({ ...modal, servers: [...modal.servers, e.target.value] })
              }
            }}
          >
            <option value="">+ добавить сервер в цепочку…</option>
            {servers
              .filter((s) => !modal.servers.includes(s.id))
              .map((s) => (
                <option key={s.id} value={s.id}>{serverLabel(s.id)}</option>
              ))}
          </select>

          <div style={{ display: 'flex', gap: 12, marginTop: 10, flexWrap: 'wrap' }}>
            <label className="check" style={{ display: 'inline-flex', gap: 6, alignItems: 'center', fontSize: 13 }}>
              Порог пинга:
              <NumberInput
                min={0}
                max={5000}
                step={50}
                fallback={0}
                value={modal.threshold}
                onChange={(val) => onChange({ ...modal, threshold: val })}
              />
              мс
            </label>
            <label className="check" style={{ display: 'inline-flex', gap: 6, alignItems: 'center', fontSize: 13 }}>
              <input
                type="checkbox"
                checked={modal.autoRestore}
                onChange={(e) => onChange({ ...modal, autoRestore: e.target.checked })}
              />
              автовозврат на основной
            </label>
          </div>
        </div>

        {/* СЕКЦИЯ 2: ПЕРСОНАЛЬНЫЕ ПРАВИЛА ДЛЯ ДОМЕНОВ */}
        <div style={{ background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border)', borderRadius: 8, padding: 12, marginBottom: 14 }}>
          <b style={{ fontSize: 13, display: 'block', marginBottom: 4 }}>🎯 Персональные правила доменов</b>
          <p className="muted small" style={{ margin: '0 0 8px' }}>
            Направлять конкретные сайты с этого устройства через отдельный сервер, DIRECT или REJECT.
          </p>

          <div className="modal-list" style={{ maxHeight: 150, marginBottom: 8 }}>
            {domainRules.length === 0 && (
              <p className="muted small" style={{ margin: '4px 0' }}>Нет персональных правил для доменов.</p>
            )}
            {domainRules.map((r, i) => (
              <div key={i} className="check-row" style={{ display: 'flex', gap: 8, alignItems: 'center', padding: '4px 8px' }}>
                <span style={{ fontFamily: 'Consolas, monospace', fontSize: 12.5, flex: 1, color: 'var(--accent)' }}>
                  {r.domain}
                </span>
                <span className="badge" style={{ fontSize: 11 }}>
                  → {r.target}
                </span>
                <button
                  type="button"
                  className="btn sm ghost"
                  onClick={() => handleRemoveDomainRule(i)}
                  title="Удалить правило"
                >
                  ✕
                </button>
              </div>
            ))}
          </div>

          <form onSubmit={handleAddDomainRule} style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
            <input
              className="input sm"
              placeholder="Домен (напр. youtube.com)"
              value={newDomain}
              onChange={(e) => setNewDomain(e.target.value)}
              style={{ flex: '1 1 180px' }}
            />
            <select
              className="select sm"
              value={newTarget}
              onChange={(e) => setNewTarget(e.target.value)}
              style={{ width: 140 }}
            >
              <option value="DIRECT">DIRECT (Напрямую)</option>
              <option value="PROXY">PROXY (По умолчанию)</option>
              <option value="REJECT">REJECT (Блокировать)</option>
              {servers.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
            <button type="submit" className="btn sm" disabled={!newDomain.trim()}>
              ＋ Добавить
            </button>
          </form>
        </div>

        {/* КНОПКИ ДЕЙСТВИЙ */}
        <div className="modal-actions" style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 10 }}>
          <button type="button" className="btn ghost" onClick={onClose}>Отмена</button>
          {hasExisting && (
            <button
              type="button"
              className="btn ghost"
              disabled={busy}
              onClick={() => onChange({ ...modal, servers: [] })}
            >
              Сбросить цепочку
            </button>
          )}
          <button type="button" className="btn primary" disabled={busy || savingRules} onClick={handleSaveAll}>
            {busy || savingRules ? 'Сохранение…' : '💾 Сохранить всё'}
          </button>
        </div>
      </div>
    </div>
  )
}
