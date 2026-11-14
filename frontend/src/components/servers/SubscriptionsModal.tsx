import type { Dispatch, FormEvent, SetStateAction } from 'react'
import type { ProviderInfo } from '../../types'
import { formatQuotaBytes } from './serversHelpers'

interface SubscriptionsModalProps {
  providersOpen: boolean
  setProvidersOpen: Dispatch<SetStateAction<boolean>>
  providers: ProviderInfo[]
  providersLoading: boolean
  providersLoaded: boolean
  providersError: boolean
  confirmDeleteSub: { id: string; name: string } | null
  setConfirmDeleteSub: Dispatch<SetStateAction<{ id: string; name: string } | null>>
  deletingSub: boolean
  editingAliases: Record<string, string>
  setEditingAliases: Dispatch<SetStateAction<Record<string, string>>>
  togglingProvider: string | null
  updatingProvider: string | null
  updatingAllProviders: boolean
  urlVisibility: Record<string, boolean>
  setUrlVisibility: Dispatch<SetStateAction<Record<string, boolean>>>
  copiedUrlId: string | null
  editingProviderNameId: string | null
  setEditingProviderNameId: Dispatch<SetStateAction<string | null>>
  editingHwidId: string | null
  setEditingHwidId: Dispatch<SetStateAction<string | null>>
  hwidInput: string
  setHwidInput: Dispatch<SetStateAction<string>>
  copiedHwidId: string | null
  savingHwidId: string | null
  newSubOpen: boolean
  setNewSubOpen: Dispatch<SetStateAction<boolean>>
  newSubId: string
  setNewSubId: Dispatch<SetStateAction<string>>
  newSubUrl: string
  setNewSubUrl: Dispatch<SetStateAction<string>>
  newSubName: string
  setNewSubName: Dispatch<SetStateAction<string>>
  newSubHwid: string
  setNewSubHwid: Dispatch<SetStateAction<string>>
  newSubAppendHwid: boolean
  setNewSubAppendHwid: Dispatch<SetStateAction<boolean>>
  addingSub: boolean
  providerColorMap: Map<string, string>
  togglingWhitelistSub: string | null
  loadProviders: () => void
  saveProviderAlias: (id: string, alias: string) => void
  toggleProviderEnable: (id: string, currentEnabled: boolean) => void
  toggleProviderWhitelist: (provId: string, currentWhitelist: boolean) => void
  updateProviderNow: (id: string) => void
  updateAllProvidersNow: () => void
  copySubUrl: (id: string, url: string) => void
  generateHwid: () => void
  handleAddSubscription: (e: FormEvent) => void
  handleSaveHwid: (id: string, hwid: string) => void
  handleCopyHwid: (id: string, hwid: string) => void
  handleDeleteSubscription: (id: string, name: string) => void
  executeDeleteSubscription: (id: string, name: string) => void
}

export function SubscriptionsModal({
  providersOpen,
  setProvidersOpen,
  providers,
  providersLoading,
  providersLoaded,
  providersError,
  confirmDeleteSub,
  setConfirmDeleteSub,
  deletingSub,
  editingAliases,
  setEditingAliases,
  togglingProvider,
  updatingProvider,
  updatingAllProviders,
  urlVisibility,
  setUrlVisibility,
  copiedUrlId,
  editingProviderNameId,
  setEditingProviderNameId,
  editingHwidId,
  setEditingHwidId,
  hwidInput,
  setHwidInput,
  copiedHwidId,
  savingHwidId,
  newSubOpen,
  setNewSubOpen,
  newSubId,
  setNewSubId,
  newSubUrl,
  setNewSubUrl,
  newSubName,
  setNewSubName,
  newSubHwid,
  setNewSubHwid,
  newSubAppendHwid,
  setNewSubAppendHwid,
  addingSub,
  providerColorMap,
  togglingWhitelistSub,
  loadProviders,
  saveProviderAlias,
  toggleProviderEnable,
  toggleProviderWhitelist,
  updateProviderNow,
  updateAllProvidersNow,
  copySubUrl,
  generateHwid,
  handleAddSubscription,
  handleSaveHwid,
  handleCopyHwid,
  handleDeleteSubscription,
  executeDeleteSubscription,
}: SubscriptionsModalProps) {
  if (!providersOpen) return null
  return (
        <div className="modal-overlay" onClick={() => setProvidersOpen(false)}>
          <div className="modal" style={{ width: 'min(720px, 94vw)' }} onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <h2>📦 Управление подписками</h2>
              <div style={{ display: 'flex', gap: 6 }}>
                <button
                  type="button"
                  className="btn sm"
                  onClick={updateAllProvidersNow}
                  disabled={updatingAllProviders}
                  title="Принудительно обновить все подписки"
                >
                  {updatingAllProviders ? 'Обновление…' : '↻ Обновить все'}
                </button>
                <button
                  type="button"
                  className="btn sm ghost"
                  onClick={() => setProvidersOpen(false)}
                  style={{ fontSize: 16, padding: '2px 8px' }}
                >
                  ✕
                </button>
              </div>
            </div>

            <p className="muted small" style={{ margin: '0 0 8px' }}>
              Вы можете переименовывать подписки, копировать их URL, добавлять новые или удалять неиспользуемые.
            </p>

            <div className="modal-list" style={{ maxHeight: '56vh', display: 'flex', flexDirection: 'column', gap: 12 }}>
              {providersError && (
                <div role="alert">
                  <span>Не удалось загрузить список подписок.{providersLoaded ? ' Показаны последние загруженные данные.' : ''}</span>{' '}
                  <button type="button" className="btn sm" onClick={loadProviders} disabled={providersLoading}>
                    {providersLoading ? 'Загрузка…' : 'Повторить'}
                  </button>
                </div>
              )}
              {providersLoading && !providersLoaded && !providersError && <p className="muted">Загрузка подписок…</p>}
              {providersLoaded && !providersError && providers.length === 0 && <p className="muted">Нет активных подписок.</p>}
              {providers.map((p) => {
                const draft = editingAliases[p.id] ?? (p.name === p.id ? '' : p.name)
                const dotColor = providerColorMap.get(p.id) || '#a855f7'
                const showUrl = urlVisibility[p.id]
                const isConfirming = confirmDeleteSub?.id === p.id

                return (
                  <div
                    key={p.id}
                    className="provider-edit-card"
                    style={{
                      background: 'rgba(255, 255, 255, 0.025)',
                      border: isConfirming ? '1px solid rgba(239, 68, 68, 0.5)' : '1px solid var(--border)',
                      borderRadius: 10,
                      padding: '12px 14px',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 10,
                      transition: 'border-color 0.2s, box-shadow 0.2s',
                    }}
                  >
                    {/* Заголовок подписки и действия */}
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 10, flexWrap: 'wrap' }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                        <span
                          className="sub-marker-dot"
                          style={{
                            backgroundColor: dotColor,
                            width: 10,
                            height: 10,
                            borderRadius: '50%',
                            display: 'inline-block',
                            boxShadow: `0 0 8px ${dotColor}66`,
                          }}
                        />
                        {editingProviderNameId === p.id ? (
                          <div style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
                            <input
                              className="input sm"
                              style={{ width: 170, height: 26, fontSize: 13, padding: '2px 8px' }}
                              value={draft}
                              onChange={(e) =>
                                setEditingAliases((prev) => ({ ...prev, [p.id]: e.target.value }))
                              }
                              autoFocus
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') {
                                  saveProviderAlias(p.id, draft)
                                  setEditingProviderNameId(null)
                                }
                                if (e.key === 'Escape') setEditingProviderNameId(null)
                              }}
                            />
                            <button
                              type="button"
                              className="btn sm primary"
                              style={{ padding: '0 6px', height: 26 }}
                              onClick={() => {
                                saveProviderAlias(p.id, draft)
                                setEditingProviderNameId(null)
                              }}
                              title="Сохранить имя"
                            >
                              ✓
                            </button>
                            <button
                              type="button"
                              className="btn sm ghost"
                              style={{ padding: '0 6px', height: 26 }}
                              onClick={() => setEditingProviderNameId(null)}
                              title="Отмена"
                            >
                              ✕
                            </button>
                          </div>
                        ) : (
                          <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
                            <b
                              style={{ fontSize: 14, cursor: 'pointer' }}
                              onClick={() => {
                                setEditingAliases((prev) => ({ ...prev, [p.id]: p.name === p.id ? '' : p.name }))
                                setEditingProviderNameId(p.id)
                              }}
                              title="Нажмите для переименования"
                            >
                              {p.name}
                            </b>
                            <button
                              type="button"
                              className="btn sm ghost"
                              style={{ padding: '0 4px', fontSize: 11, height: 20, opacity: 0.7 }}
                              onClick={() => {
                                setEditingAliases((prev) => ({ ...prev, [p.id]: p.name === p.id ? '' : p.name }))
                                setEditingProviderNameId(p.id)
                              }}
                              title="Переименовать подписку"
                            >
                              ✏️
                            </button>
                          </div>
                        )}
                        {p.is_whitelist && (
                          <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.25)', color: '#c084fc', border: '1px solid rgba(168, 85, 247, 0.5)', fontSize: 10, fontWeight: 600 }}>
                            🟣 Белый список
                          </span>
                        )}
                        <span
                          style={{
                            fontFamily: 'Consolas, monospace',
                            fontSize: 11,
                            background: 'rgba(255,255,255,0.06)',
                            padding: '1px 6px',
                            borderRadius: 4,
                            color: 'var(--muted)',
                          }}
                        >
                          {p.id}
                        </span>
                        <span
                          style={{
                            fontSize: 11,
                            background: 'rgba(56, 189, 248, 0.1)',
                            color: '#38bdf8',
                            padding: '1px 7px',
                            borderRadius: 12,
                            fontWeight: 500,
                          }}
                        >
                          {p.count} серв.
                        </span>
                        {p.updated_at && (
                          <span style={{ fontSize: 11, color: 'var(--muted)' }}>
                            · обновл. {p.updated_at.slice(0, 10)}
                          </span>
                        )}
                      </div>

                      {/* Кнопки переключения, обновления, белых списков и удаления */}
                      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                        <button
                          type="button"
                          className={`btn sm ${p.enabled !== false ? 'primary' : 'ghost'}`}
                          style={{
                            fontWeight: 600,
                            minWidth: 90,
                            background: p.enabled !== false ? 'rgba(34, 197, 94, 0.2)' : 'rgba(255, 255, 255, 0.05)',
                            borderColor: p.enabled !== false ? 'rgba(34, 197, 94, 0.6)' : 'var(--border)',
                            color: p.enabled !== false ? '#4ade80' : 'var(--muted)',
                          }}
                          title={p.enabled !== false ? 'Отключить подписку (серверы будут скрыты)' : 'Включить подписку'}
                          disabled={togglingProvider === p.id || deletingSub}
                          onClick={() => toggleProviderEnable(p.id, p.enabled !== false)}
                        >
                          {togglingProvider === p.id ? '⏳…' : p.enabled !== false ? '● Активна' : '○ Отключена'}
                        </button>
                        <button
                          type="button"
                          className={`btn sm ghost btn-whitelist ${p.is_whitelist ? 'is-whitelist' : ''}`}
                          style={{
                            color: p.is_whitelist ? '#c084fc' : undefined,
                            borderColor: p.is_whitelist ? 'rgba(168, 85, 247, 0.5)' : undefined,
                            background: p.is_whitelist ? 'rgba(168, 85, 247, 0.15)' : undefined,
                            fontWeight: p.is_whitelist ? 600 : 400,
                          }}
                          title={p.is_whitelist ? 'Исключить подписку из белых списков' : 'Включить всю подписку в белые списки (100% туннель)'}
                          disabled={togglingWhitelistSub === p.id || deletingSub}
                          onClick={() => toggleProviderWhitelist(p.id, Boolean(p.is_whitelist))}
                        >
                          {togglingWhitelistSub === p.id ? '⏳…' : p.is_whitelist ? '🟣 В белом списке' : '⚪ Белые списки'}
                        </button>
                        <button
                          type="button"
                          className="btn sm ghost"
                          title="Загрузить свежие серверы из этой подписки"
                          disabled={updatingProvider === p.id || deletingSub}
                          onClick={() => updateProviderNow(p.id)}
                        >
                          {updatingProvider === p.id ? '⏳ Обновление…' : '↻ Обновить'}
                        </button>
                        <button
                          type="button"
                          className="btn sm"
                          style={{
                            color: '#f87171',
                            background: isConfirming ? 'rgba(239,68,68,0.2)' : 'rgba(239, 68, 68, 0.08)',
                            border: '1px solid rgba(239,68,68,0.3)',
                            display: 'inline-flex',
                            alignItems: 'center',
                            gap: 5,
                          }}
                          title="Удалить подписку"
                          disabled={deletingSub}
                          onClick={() => handleDeleteSubscription(p.id, p.name)}
                        >
                          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ flexShrink: 0 }}>
                            <polyline points="3 6 5 6 21 6" />
                            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                          </svg>
                          Удалить
                        </button>
                      </div>
                    </div>

                    {/* Баннер подтверждения удаления */}
                    {isConfirming && (
                      <div
                        style={{
                          padding: '10px 12px',
                          background: 'rgba(239, 68, 68, 0.12)',
                          border: '1px solid rgba(239, 68, 68, 0.35)',
                          borderRadius: 8,
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 10,
                          flexWrap: 'wrap',
                        }}
                      >
                        <span style={{ fontSize: 12.5, color: '#fca5a5' }}>
                          Удалить подписку <b>«{p.name}»</b> из config.yaml и всех групп маршрутизации?
                        </span>
                        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                          <button
                            type="button"
                            className="btn sm"
                            style={{ background: '#dc2626', color: '#fff', border: 'none' }}
                            disabled={deletingSub}
                            onClick={() => executeDeleteSubscription(p.id, p.name)}
                          >
                            {deletingSub ? 'Удаление…' : 'Да, удалить'}
                          </button>
                          <button
                            type="button"
                            className="btn sm ghost"
                            disabled={deletingSub}
                            onClick={() => setConfirmDeleteSub(null)}
                          >
                            Отмена
                          </button>
                        </div>
                      </div>
                    )}

                    {/* Виджет квоты трафика подписки (если провайдер отдал Subscription-Userinfo) */}
                    {p.total_bytes && p.total_bytes > 0 ? (
                      <div
                        style={{
                          background: 'rgba(56, 189, 248, 0.05)',
                          border: '1px solid rgba(56, 189, 248, 0.18)',
                          borderRadius: 8,
                          padding: '8px 12px',
                          fontSize: 11.5,
                        }}
                      >
                        {(() => {
                          const used = (p.upload_bytes || 0) + (p.download_bytes || 0)
                          const total = p.total_bytes!
                          const percent = Math.min(100, Math.round((used / total) * 100))
                          const remaining = Math.max(0, total - used)
                          const expireStr = p.expire_timestamp
                            ? new Date(p.expire_timestamp * 1000).toLocaleDateString()
                            : null

                          return (
                            <div style={{ display: 'flex', flexDirection: 'column', gap: 5 }}>
                              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
                                <span style={{ color: '#38bdf8', fontWeight: 600 }}>
                                  📊 Лимит трафика: {formatQuotaBytes(used)} / {formatQuotaBytes(total)} ({percent}%)
                                </span>
                                <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                                  <span style={{ color: remaining === 0 ? '#ef4444' : remaining < total * 0.1 ? '#fbbf24' : '#4ade80', fontWeight: 600 }}>
                                    Осталось: {formatQuotaBytes(remaining)}
                                  </span>
                                  {expireStr && (
                                    <span className="muted" style={{ fontSize: 10.5 }}>
                                      · До {expireStr}
                                    </span>
                                  )}
                                </div>
                              </div>
                              <div
                                style={{
                                  width: '100%',
                                  height: 4,
                                  background: 'rgba(255,255,255,0.08)',
                                  borderRadius: 2,
                                  overflow: 'hidden',
                                }}
                              >
                                <div
                                  style={{
                                    width: `${percent}%`,
                                    height: '100%',
                                    background: percent > 90 ? '#ef4444' : percent > 75 ? '#fbbf24' : '#38bdf8',
                                    borderRadius: 2,
                                    transition: 'width 0.3s ease',
                                  }}
                                />
                              </div>
                            </div>
                          )
                        })()}
                      </div>
                    ) : null}

                    {/* Единый аккуратный блок реквизитов подписки (HWID + URL) */}
                    <div
                      className="provider-props-box"
                      style={{
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 6,
                        background: 'rgba(0, 0, 0, 0.22)',
                        border: '1px solid rgba(255, 255, 255, 0.05)',
                        borderRadius: 8,
                        padding: '8px 10px',
                      }}
                    >
                      {/* Строка HWID */}
                      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, flexWrap: 'wrap', minHeight: 28 }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flex: 1, minWidth: 200 }}>
                          <span style={{ fontSize: 11, color: '#38bdf8', fontWeight: 600, display: 'flex', alignItems: 'center', gap: 4 }}>
                            🔑 HWID:
                          </span>
                          {editingHwidId === p.id ? (
                            <div style={{ display: 'flex', alignItems: 'center', gap: 4, flex: 1 }}>
                              <input
                                className="input sm"
                                style={{ flex: 1, height: 24, fontSize: 11.5, fontFamily: 'Consolas, monospace', padding: '2px 6px' }}
                                value={hwidInput}
                                onChange={(e) => setHwidInput(e.target.value)}
                                placeholder="keenetic-router-xxx или ваш HWID"
                                autoFocus
                                onKeyDown={(e) => {
                                  if (e.key === 'Enter') handleSaveHwid(p.id, hwidInput)
                                  if (e.key === 'Escape') setEditingHwidId(null)
                                }}
                              />
                              <button
                                type="button"
                                className="btn sm primary"
                                style={{ height: 24, padding: '0 6px', fontSize: 11 }}
                                disabled={savingHwidId === p.id}
                                onClick={() => handleSaveHwid(p.id, hwidInput)}
                                title="Сохранить HWID"
                              >
                                ✓
                              </button>
                              <button
                                type="button"
                                className="btn sm ghost"
                                style={{ height: 24, padding: '0 6px', fontSize: 11 }}
                                onClick={() => setEditingHwidId(null)}
                                title="Отмена"
                              >
                                ✕
                              </button>
                            </div>
                          ) : p.hwid ? (
                            <span
                              style={{
                                fontFamily: 'Consolas, monospace',
                                fontSize: 11,
                                color: '#38bdf8',
                                background: 'rgba(56, 189, 248, 0.12)',
                                padding: '1px 7px',
                                borderRadius: 4,
                                border: '1px solid rgba(56, 189, 248, 0.25)',
                              }}
                            >
                              {p.hwid}
                            </span>
                          ) : (
                            <button
                              type="button"
                              className="btn sm ghost"
                              style={{ fontSize: 11, padding: '1px 6px', color: '#38bdf8', height: 22, borderStyle: 'dashed' }}
                              onClick={() => {
                                setEditingHwidId(p.id)
                                setHwidInput('')
                              }}
                              title="Привязать уникальный идентификатор роутера к подписке"
                            >
                              ➕ Привязать HWID
                            </button>
                          )}
                        </div>

                        {editingHwidId !== p.id && p.hwid && (
                          <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                            <button
                              type="button"
                              className="btn sm ghost"
                              style={{ padding: '2px 8px', fontSize: 11 }}
                              onClick={() => handleCopyHwid(p.id, p.hwid!)}
                              title="Скопировать HWID в буфер обмена"
                            >
                              {copiedHwidId === p.id ? '✓ Скопирован' : '📋 Скопировать HWID'}
                            </button>
                            <button
                              type="button"
                              className="btn sm ghost"
                              style={{ padding: '2px 6px', fontSize: 11 }}
                              onClick={() => {
                                setEditingHwidId(p.id)
                                setHwidInput(p.hwid || '')
                              }}
                              title="Изменить HWID"
                            >
                              ✏️
                            </button>
                            <button
                              type="button"
                              className="btn sm ghost"
                              style={{ padding: '2px 6px', fontSize: 11, color: '#ef4444' }}
                              disabled={savingHwidId === p.id}
                              onClick={() => handleSaveHwid(p.id, '')}
                              title="Удалить HWID из подписки"
                            >
                              🗑
                            </button>
                          </div>
                        )}
                      </div>

                      {/* Строка URL */}
                      {p.url && (
                        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, flexWrap: 'wrap', paddingTop: 4, borderTop: '1px solid rgba(255,255,255,0.04)' }}>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 6, flex: 1, minWidth: 200, overflow: 'hidden' }}>
                            <span className="muted small" style={{ fontSize: 11, flexShrink: 0, fontWeight: 600 }}>
                              URL:
                            </span>
                            <span
                              style={{
                                fontFamily: 'Consolas, monospace',
                                fontSize: 11.5,
                                color: showUrl ? 'var(--text)' : 'var(--muted)',
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                whiteSpace: 'nowrap',
                              }}
                              title={showUrl ? p.url : 'Нажмите «Показать» для просмотра полного URL'}
                            >
                              {showUrl ? p.url : '••••••••••••••••••••••••••••••••••••••••••••'}
                            </span>
                          </div>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                            <button
                              type="button"
                              className="btn sm ghost"
                              onClick={() => setUrlVisibility((prev) => ({ ...prev, [p.id]: !showUrl }))}
                              title={showUrl ? 'Скрыть URL' : 'Показать URL'}
                              style={{ padding: '2px 8px', fontSize: 11 }}
                            >
                              {showUrl ? '🙈 Скрыть' : '👁 Показать'}
                            </button>
                            <button
                              type="button"
                              className="btn sm ghost"
                              onClick={() => copySubUrl(p.id, p.url!)}
                              title="Скопировать URL в буфер обмена"
                              style={{ padding: '2px 8px', fontSize: 11 }}
                            >
                              {copiedUrlId === p.id ? '✓ Скопировано' : '📋 Копировать'}
                            </button>
                          </div>
                        </div>
                      )}
                    </div>
                  </div>
                )
              })}

              {/* Форма добавления новой подписки */}
              <div style={{ marginTop: 12, padding: '12px 14px', background: 'rgba(255,255,255,0.03)', border: '1px solid var(--border)', borderRadius: 8 }}>
                {!newSubOpen ? (
                  <button
                    type="button"
                    className="btn sm ghost"
                    onClick={() => {
                      setNewSubOpen(true)
                      if (!newSubHwid) generateHwid()
                    }}
                    style={{ width: '100%', color: 'var(--accent)' }}
                  >
                    ➕ Добавить новую подписку (с HWID / https sub)
                  </button>
                ) : (
                  <form onSubmit={handleAddSubscription} style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                      <b style={{ fontSize: 13, color: 'var(--accent)' }}>➕ Добавление подписки с HWID</b>
                      <span className="muted small">HTTPS Sub & Mihomo Provider</span>
                    </div>

                    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                      <input
                        className="input sm"
                        placeholder="ID (латиница, напр. geodema, sub_main)"
                        value={newSubId}
                        onChange={(e) => setNewSubId(e.target.value)}
                        style={{ flex: '1 1 180px' }}
                        required
                      />
                      <input
                        className="input sm"
                        placeholder="Отображаемое имя (напр. Основная подписка)"
                        value={newSubName}
                        onChange={(e) => setNewSubName(e.target.value)}
                        style={{ flex: '1 1 180px' }}
                      />
                    </div>

                    <input
                      className="input sm"
                      placeholder="URL подписки (https://domain.com/sub/...)"
                      value={newSubUrl}
                      onChange={(e) => {
                        const val = e.target.value
                        setNewSubUrl(val)
                        if (!newSubId && val.startsWith('http')) {
                          try {
                            const u = new URL(val)
                            const seg = u.pathname.split('/').filter(Boolean).pop()
                            const cleanHost = u.hostname.replace(/^(sub|account|api|vpn)\./, '').split('.')[0]
                            setNewSubId((cleanHost || seg || 'sub').replace(/[^a-zA-Z0-9_]/g, '').toLowerCase())
                          } catch {}
                        }
                      }}
                      required
                    />

                    {/* Поле HWID и генератор */}
                    <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                      <input
                        className="input sm"
                        placeholder="HWID устройства (напр. keenetic-router-228)"
                        value={newSubHwid}
                        onChange={(e) => setNewSubHwid(e.target.value)}
                        style={{ flex: 1, fontFamily: 'Consolas, monospace' }}
                      />
                      <button
                        type="button"
                        className="btn sm ghost"
                        onClick={generateHwid}
                        title="Сгенерировать случайный HWID"
                        style={{ whiteSpace: 'nowrap' }}
                      >
                        🎲 Сгенерировать
                      </button>
                    </div>

                    {/* Опция добавления HWID в URL */}
                    <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 12, cursor: 'pointer', userSelect: 'none' }}>
                      <input
                        type="checkbox"
                        checked={newSubAppendHwid}
                        onChange={(e) => setNewSubAppendHwid(e.target.checked)}
                      />
                      <span>Добавить параметр <code style={{ color: '#38bdf8' }}>?hwid=...</code> в URL подписки (формат https sub)</span>
                    </label>

                    <div style={{ fontSize: 11, color: 'var(--muted)', lineHeight: 1.4 }}>
                      💡 Заголовок <code style={{ color: '#38bdf8' }}>x-hwid</code> и User-Agent ClashMeta прописываются автоматически в <code style={{ color: 'var(--accent)' }}>config.yaml</code>.
                    </div>

                    <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6, marginTop: 4 }}>
                      <button type="button" className="btn sm ghost" onClick={() => setNewSubOpen(false)}>
                        Отмена
                      </button>
                      <button type="submit" className="btn sm primary" disabled={addingSub}>
                        {addingSub ? 'Добавление…' : 'Добавить подписку'}
                      </button>
                    </div>
                  </form>
                )}
              </div>
            </div>

            {/* Липкий футер */}
            <div className="modal-actions">
              <button className="btn primary" onClick={() => setProvidersOpen(false)}>
                Закрыть
              </button>
            </div>
          </div>
        </div>
  )
}