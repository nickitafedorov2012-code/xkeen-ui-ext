import { pingClass, type ServerInfo } from '../types'

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

/// Модалка «Сервер и резервы» (цепочка failover устройства) —
/// вынесено из Devices.tsx.
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
  const drMove = (idx: number, dir: -1 | 1) => {
    const next = [...modal.servers]
    const j = idx + dir
    if (j < 0 || j >= next.length) return
    ;[next[idx], next[j]] = [next[j], next[idx]]
    onChange({ ...modal, servers: next })
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>🛡️ Сервер и резервы — {modal.name}</h2>
        <p className="muted small">
          Цепочка: первый — основной, остальные — резервы. Если основной отвалился или пинг выше порога,
          устройство переключается на следующий живой. При восстановлении основного — автовозврат.
        </p>
        <div className="modal-list">
          {modal.servers.length === 0 && <p className="muted">Цепочка пуста — устройство использует PROXY по умолчанию.</p>}
          {modal.servers.map((id, i) => {
            const s = servers.find((x) => x.id === id)
            return (
              <div key={id} className="check-row" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                <span className="badge">{i === 0 ? 'ОСН' : `РЕЗ${i}`}</span>
                <span className="server-name" style={{ flex: 1 }} title={id}>{serverLabel(id)}</span>
                {s && <span className={'ping ' + pingClass(s.ping_ms)}>{s.ping_ms > 0 ? `${s.ping_ms} мс` : '—'}</span>}
                <button className="btn sm ghost" disabled={i === 0} onClick={() => drMove(i, -1)}>↑</button>
                <button className="btn sm ghost" disabled={i === modal.servers.length - 1} onClick={() => drMove(i, 1)}>↓</button>
                <button
                  className="btn sm ghost"
                  onClick={() => onChange({ ...modal, servers: modal.servers.filter((x) => x !== id) })}
                >
                  ✕
                </button>
              </div>
            )
          })}
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
        </div>
        <div className="modal-actions" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 8 }}>
          <label className="check" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
            Порог пинга, мс:
            <input
              className="input"
              type="number"
              min={50}
              max={5000}
              step={50}
              value={modal.threshold}
              onChange={(e) => onChange({ ...modal, threshold: Number(e.target.value) || 0 })}
            />
          </label>
          <label className="check" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
            <input
              type="checkbox"
              checked={modal.autoRestore}
              onChange={(e) => onChange({ ...modal, autoRestore: e.target.checked })}
            />
            автовозврат на основной, когда восстановится
          </label>
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button className="btn ghost" onClick={onClose}>Отмена</button>
            {hasExisting && (
              <button
                className="btn ghost"
                disabled={busy}
                onClick={() => onChange({ ...modal, servers: [] })}
              >
                Снять маршрутизацию
              </button>
            )}
            <button className="btn primary" disabled={busy} onClick={onSave}>
              {busy ? 'Применение…' : 'Сохранить'}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
