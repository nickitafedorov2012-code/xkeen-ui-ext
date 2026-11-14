import type { Dispatch, SetStateAction } from 'react'
import { pingClass, type ProviderInfo, type ServerInfo } from '../../types'

interface IgnoreListModalProps {
  ignoreOpen: boolean
  setIgnoreOpen: Dispatch<SetStateAction<boolean>>
  providers: ProviderInfo[]
  providerColorMap: Map<string, string>
  ignoreCandidates: ServerInfo[]
  filteredIgnoreCandidates: ServerInfo[]
  ignoreDraft: Set<string>
  setIgnoreDraft: Dispatch<SetStateAction<Set<string>>>
  initialIgnored: Set<string>
  ignoreSearch: string
  setIgnoreSearch: Dispatch<SetStateAction<string>>
  ignoreProviderFilter: string
  setIgnoreProviderFilter: Dispatch<SetStateAction<string>>
  ignoreSaving: boolean
  toggleIgnoreDraft: (id: string, on: boolean) => void
  selectAllFilteredIgnore: () => void
  unselectAllFilteredIgnore: () => void
  saveIgnore: () => void
}

export function IgnoreListModal({
  ignoreOpen,
  setIgnoreOpen,
  providers,
  providerColorMap,
  ignoreCandidates,
  filteredIgnoreCandidates,
  ignoreDraft,
  setIgnoreDraft,
  initialIgnored,
  ignoreSearch,
  setIgnoreSearch,
  ignoreProviderFilter,
  setIgnoreProviderFilter,
  ignoreSaving,
  toggleIgnoreDraft,
  selectAllFilteredIgnore,
  unselectAllFilteredIgnore,
  saveIgnore,
}: IgnoreListModalProps) {
  if (!ignoreOpen) return null
  return (
        <div className="modal-overlay" onClick={() => setIgnoreOpen(false)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
              <h2>🚫 Игнор-лист серверов</h2>
              <button
                type="button"
                className="btn sm ghost"
                onClick={() => setIgnoreOpen(false)}
                style={{ fontSize: 16, padding: '2px 8px' }}
              >
                ✕
              </button>
            </div>
            <p className="muted small" style={{ margin: '0 0 6px' }}>
              Отмеченные серверы исключаются из авто-групп <b>Fastest</b> и <b>Fallback</b>. Ручное подключение остаётся доступным.
            </p>

            {/* Поиск и фильтр по подписке внутри модалки */}
            <div className="modal-filter-row">
              <input
                className="input sm"
                placeholder="Поиск серверов в модалке…"
                value={ignoreSearch}
                onChange={(e) => setIgnoreSearch(e.target.value)}
                style={{ flex: 1 }}
              />
              {providers.length > 0 && (
                <select
                  className="input sm"
                  value={ignoreProviderFilter}
                  onChange={(e) => setIgnoreProviderFilter(e.target.value)}
                  style={{ maxWidth: 180 }}
                >
                  <option value="">Все подписки ({ignoreCandidates.length})</option>
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} ({p.count})
                    </option>
                  ))}
                  <option value="__static__">Статические</option>
                </select>
              )}
              <button
                type="button"
                className="btn sm ghost"
                onClick={selectAllFilteredIgnore}
                title="Отметить все отфильтрованные серверы"
              >
                Выбрать все
              </button>
              <button
                type="button"
                className="btn sm ghost"
                onClick={unselectAllFilteredIgnore}
                title="Снять отметки с отфильтрованных"
              >
                Снять все
              </button>
            </div>

            {/* Список серверов */}
            <div className="modal-list">
              {filteredIgnoreCandidates.length === 0 ? (
                <p className="muted">Серверов не найдено.</p>
              ) : (
                filteredIgnoreCandidates.map((s) => {
                  const isChecked = ignoreDraft.has(s.id)
                  const wasInitiallyChecked = initialIgnored.has(s.id)
                  return (
                    <label key={s.id} className="check-row">
                      <input
                        type="checkbox"
                        checked={isChecked}
                        onChange={(e) => toggleIgnoreDraft(s.id, e.target.checked)}
                      />
                      <span className="badge">{s.protocol}</span>
                      {s.provider && (
                        <span
                          className="sub-marker-dot"
                          style={{ backgroundColor: providerColorMap.get(s.provider) || '#a855f7' }}
                        />
                      )}
                      <span className="server-name" title={s.name}>
                        {s.name}
                      </span>
                      {/* Индикация изменений сессии */}
                      {isChecked && !wasInitiallyChecked && (
                        <span className="diff-added-badge">+добавлен</span>
                      )}
                      {!isChecked && wasInitiallyChecked && (
                        <span className="diff-removed-badge">-снят</span>
                      )}
                      {s.ping_ms > 0 && (
                        <span className={'ping ' + pingClass(s.ping_ms)}>{s.ping_ms} мс</span>
                      )}
                    </label>
                  )
                })
              )}
            </div>

            {/* Липкий футер (Sticky actions) */}
            <div className="modal-actions">
              <button className="btn" onClick={() => setIgnoreOpen(false)}>
                Отмена
              </button>
              <button
                className="btn ghost"
                disabled={ignoreSaving || ignoreDraft.size === 0}
                onClick={() => setIgnoreDraft(new Set())}
              >
                Сбросить всё
              </button>
              <button className="btn primary" onClick={saveIgnore} disabled={ignoreSaving}>
                {ignoreSaving ? 'Применение…' : `Применить (${ignoreDraft.size})`}
              </button>
            </div>
          </div>
        </div>
  )
}