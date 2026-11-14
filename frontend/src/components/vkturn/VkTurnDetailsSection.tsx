import React from 'react'
import type { VkTurnPhase, VkTurnStatus } from '../../types'

export interface VkTurnDetailsSectionProps {
  status: VkTurnStatus | null
  phase: VkTurnPhase
  showPasswordFix: boolean
  accordionOpen: boolean
  setAccordionOpen: React.Dispatch<React.SetStateAction<boolean>>
  fetchLogs: () => void
  loadingLogs: boolean
  logs: string[]
  maxLinks: number
  showManualLinksSection: boolean
  newLinkInput: string
  setNewLinkInput: React.Dispatch<React.SetStateAction<string>>
  updatingLink: boolean
  handleUpdateCallLink: (addAsFallback: boolean) => void
  handleRemoveLink: (index: number) => void
  editPeerInput: string
  setEditPeerInput: React.Dispatch<React.SetStateAction<string>>
  peerEditedRef: React.MutableRefObject<boolean>
  updatingPeer: boolean
  handleUpdatePeer: () => void
  newPassword: string
  setNewPassword: React.Dispatch<React.SetStateAction<string>>
  updatingPassword: boolean
  handleUpdatePassword: () => void
  actionPending: boolean
  targetStreams: number
  displayedStreamsStr: string
  rangeStreams: number
  canSaveStreams: boolean
  streamsUpdating: boolean
  setDraftStreamsInput: React.Dispatch<React.SetStateAction<string | null>>
  handleSaveStreams: () => void
}

export function VkTurnDetailsSection({
  status,
  phase,
  showPasswordFix,
  accordionOpen,
  setAccordionOpen,
  fetchLogs,
  loadingLogs,
  logs,
  maxLinks,
  showManualLinksSection,
  newLinkInput,
  setNewLinkInput,
  updatingLink,
  handleUpdateCallLink,
  handleRemoveLink,
  editPeerInput,
  setEditPeerInput,
  peerEditedRef,
  updatingPeer,
  handleUpdatePeer,
  newPassword,
  setNewPassword,
  updatingPassword,
  handleUpdatePassword,
  actionPending,
  targetStreams,
  displayedStreamsStr,
  rangeStreams,
  canSaveStreams,
  streamsUpdating,
  setDraftStreamsInput,
  handleSaveStreams,
}: VkTurnDetailsSectionProps) {
  return (
  <section className="vk-section" style={{ padding: '18px 24px' }}>
    <div
      role="button"
      tabIndex={0}
      onClick={() => {
        const next = !accordionOpen
        setAccordionOpen(next)
        if (next && logs.length === 0) {
          fetchLogs()
        }
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          setAccordionOpen((prev) => !prev)
        }
      }}
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        cursor: 'pointer',
        userSelect: 'none',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <span style={{ fontSize: 18 }}>📋</span>
        <span style={{ fontWeight: 600, fontSize: 15 }}>Настройки туннеля и подробности</span>
      </div>
      <span style={{ fontSize: 16, transform: accordionOpen ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s' }}>
        ▼
      </span>
    </div>

    {accordionOpen && (
      <div style={{ marginTop: 18, paddingTop: 16, borderTop: '1px solid var(--border)' }}>
        <div style={{ marginBottom: 20, maxWidth: 640 }} data-testid="stream-controls">
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 8, marginBottom: 8 }}>
            <label htmlFor="vkturn-stream-range" style={{ fontSize: 13.5, fontWeight: 600 }}>
              Мощность туннеля (потоки):{' '}
              <span style={{ color: 'var(--accent, #3b82f6)', fontWeight: 700, fontSize: 16 }}>
                {displayedStreamsStr || targetStreams}
              </span>
            </label>
            <span className="badge badge-neutral" style={{ fontSize: 12 }}>
              Активно: {status?.connected_streams ?? 0} из {targetStreams}
            </span>
          </div>
          <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
            <input
              id="vkturn-stream-range"
              type="range"
              min={3}
              max={100}
              step={1}
              className="input"
              data-testid="stream-range"
              value={rangeStreams}
              disabled={streamsUpdating || !status?.enabled}
              onChange={(e) => setDraftStreamsInput(e.target.value)}
              style={{ flex: '1 1 200px', maxWidth: 300, cursor: 'pointer' }}
            />
            <input
              id="vkturn-stream-count"
              type="number"
              min={3}
              max={100}
              className="input"
              data-testid="stream-count"
              value={displayedStreamsStr}
              disabled={streamsUpdating || !status?.enabled}
              onChange={(e) => setDraftStreamsInput(e.target.value)}
              style={{ width: 85, fontWeight: 600 }}
            />
            <button
              type="button"
              className="btn btn-primary"
              data-testid="save-streams-btn"
              disabled={!canSaveStreams}
              onClick={() => handleSaveStreams()}
              style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
            >
              <span>💾 Сохранить</span>
            </button>
          </div>
          <div className="muted" style={{ fontSize: 12, marginTop: 8, lineHeight: 1.4 }}>
            Установите нужное число потоков и нажмите «Сохранить». Потоки применяются однократно без промежуточных перезапусков.
          </div>
        </div>

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
            gap: 14,
            marginBottom: 20,
          }}
        >
          {showManualLinksSection && (
            <div className="vk-subcard">
              <div className="muted" style={{ fontSize: 12 }}>Ссылки на звонки</div>
              <div style={{ fontSize: 18, fontWeight: 700, marginTop: 4 }}>
                {status?.links_count ?? 0} из {maxLinks}
              </div>
            </div>
          )}
          <div className="vk-subcard">
            <div className="muted" style={{ fontSize: 12 }}>Число перезапусков</div>
            <div style={{ fontSize: 18, fontWeight: 700, marginTop: 4 }}>{status?.restarts ?? 0}</div>
          </div>
          <div className="vk-subcard">
            <div className="muted" style={{ fontSize: 12 }}>Адрес роутера в туннеле</div>
            <div style={{ fontSize: 15, fontWeight: 600, marginTop: 4, fontFamily: 'monospace' }} data-testid="tunnel-ip">
              {status?.tunnel_ip ? `${status.tunnel_ip} · ${status.interface}` : 'выдаст сервер при подключении'}
            </div>
          </div>
        </div>

        {showManualLinksSection && (
          <div style={{ marginBottom: 20 }}>
            <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Ссылки на звонки</div>
            {(status?.links || []).length > 0 ? (
              <ul style={{ margin: '0 0 12px 0', paddingLeft: 18, fontSize: 13, fontFamily: 'monospace' }} data-testid="links-list">
                {(status?.links || []).map((l, i) => (
                  <li key={`${l}-${i}`} style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                    <span>{l}{i === 0 ? ' (основная)' : ''}</span>
                    {((status?.links?.length ?? 0) > 1 || !status?.enabled) && (
                      <button
                        type="button"
                        className="btn btn-secondary"
                        disabled={updatingLink || actionPending}
                        onClick={() => void handleRemoveLink(i)}
                        title="Удалить ссылку"
                        style={{ padding: '1px 7px', fontSize: 11 }}
                      >
                        ✕
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            ) : (
              <div className="muted" data-testid="links-list" style={{ fontSize: 12.5, marginBottom: 10 }}>
                Ссылки ещё не добавлены. Укажите рабочую ссылку на звонок ВКонтакте.
              </div>
            )}

            {phase !== 'link_invalid' && (
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap', maxWidth: 680 }}>
                <input
                  type="text"
                  className="input"
                  placeholder="Добавить ссылку (https://vk.com/call/join/...)"
                  value={newLinkInput}
                  onChange={(e) => setNewLinkInput(e.target.value)}
                  style={{ flex: '1 1 260px' }}
                />
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={updatingLink || !newLinkInput.trim()}
                  onClick={() => void handleUpdateCallLink(false)}
                  style={{ fontSize: 13 }}
                >
                  Основная
                </button>
                {(status?.links?.length ?? 0) < maxLinks && (
                  <button
                    type="button"
                    className="btn btn-secondary"
                    disabled={updatingLink || !newLinkInput.trim()}
                    onClick={() => void handleUpdateCallLink(true)}
                    style={{ fontSize: 13 }}
                  >
                    + Запасная
                  </button>
                )}
              </div>
            )}
          </div>
        )}

        {!showPasswordFix && (
          <div style={{ marginBottom: 20 }}>
            <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Адрес сервера csqtt</div>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center', maxWidth: 640, marginBottom: 16 }}>
              <input
                type="text"
                className="input"
                data-testid="accordion-peer-input"
                placeholder="host:port или ссылка csqtt://..."
                value={editPeerInput}
                onChange={(e) => {
                  peerEditedRef.current = true
                  setEditPeerInput(e.target.value)
                }}
                style={{ flex: 1 }}
              />
              <button
                type="button"
                className="btn btn-secondary"
                disabled={updatingPeer || !editPeerInput.trim()}
                onClick={handleUpdatePeer}
                style={{ fontSize: 13 }}
              >
                {updatingPeer ? 'Сохранение…' : 'Сохранить адрес'}
              </button>
            </div>

            <div data-testid="password-change">
              <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 8 }}>Пароль подключения к серверу</div>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', maxWidth: 640 }}>
                <input
                  type="password"
                  className="input"
                  autoComplete="new-password"
                  placeholder="Сменить пароль подключения"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                  style={{ flex: 1 }}
                />
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={updatingPassword || !newPassword.trim()}
                  onClick={handleUpdatePassword}
                  style={{ fontSize: 13 }}
                >
                  {updatingPassword ? 'Сохранение…' : 'Сменить пароль'}
                </button>
              </div>
            </div>
          </div>
        )}

        <div>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
            <span style={{ fontSize: 13, fontWeight: 600 }}>Логи службы (Tail)</span>
            <button
              type="button"
              className="btn btn-secondary"
              disabled={loadingLogs}
              onClick={fetchLogs}
              style={{ fontSize: 12, padding: '4px 8px' }}
            >
              {loadingLogs ? 'Обновление…' : '🔄 Обновить логи'}
            </button>
          </div>
          <pre
            data-testid="log-tail-pre"
            style={{
              background: 'var(--code-bg, #090d16)',
              color: 'var(--code-fg, #e2e8f0)',
              padding: 12,
              borderRadius: 8,
              fontSize: 12,
              fontFamily: 'Consolas, monospace',
              maxHeight: 260,
              overflowY: 'auto',
              border: '1px solid var(--border)',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-all',
            }}
          >
            {logs.length > 0 ? logs.join('\n') : 'Логи пока пусты.'}
          </pre>
        </div>
      </div>
    )}
  </section>
  )
}
