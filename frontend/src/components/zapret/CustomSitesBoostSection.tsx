import type { Dispatch, SetStateAction } from 'react'
import type { ZapretFeatures } from '../../types'
import { normalizeDomainInput, renderMicroSpinner } from './zapretHelpers'

interface CustomSitesBoostSectionProps {
  busy: boolean
  pendingKeys: Set<string>
  features: ZapretFeatures
  customDomainInput: string
  setCustomDomainInput: Dispatch<SetStateAction<string>>
  addingCustom: boolean
  boostingDomain: string | null
  expandedCdnDomains: Set<string>
  setExpandedCdnDomains: Dispatch<SetStateAction<Set<string>>>
  isRunning: boolean
  isInstalled: boolean
  handleAddCustomDomain: (domainToAdd?: string) => void
  handleRemoveCustomDomain: (domain: string) => void
  handleToggleCustomDomain: (domain: string, currentEnabled: boolean) => void
  handleToggleAllCustomDomains: () => void
  handleBoostCustomDomain: (domain: string) => void
}

export function CustomSitesBoostSection({
  busy,
  pendingKeys,
  features,
  customDomainInput,
  setCustomDomainInput,
  addingCustom,
  boostingDomain,
  expandedCdnDomains,
  setExpandedCdnDomains,
  isRunning,
  isInstalled,
  handleAddCustomDomain,
  handleRemoveCustomDomain,
  handleToggleCustomDomain,
  handleToggleAllCustomDomains,
  handleBoostCustomDomain,
}: CustomSitesBoostSectionProps) {
  return (
      <section
        className="card"
        style={{
          padding: '22px 24px',
          background: 'linear-gradient(135deg, rgba(168, 85, 247, 0.05) 0%, rgba(15, 23, 42, 0.5) 100%)',
          border: '1px solid rgba(168, 85, 247, 0.3)',
          borderRadius: 16,
          boxShadow: '0 8px 24px rgba(168, 85, 247, 0.05)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', flexWrap: 'wrap', gap: 14, marginBottom: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
              <div
                style={{
                  width: 32,
                  height: 32,
                  borderRadius: 8,
                  background: 'rgba(168, 85, 247, 0.2)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  fontSize: 16,
                }}
              >
                ⚡
              </div>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                Свой сайт & CDN Boost (/boost)
              </h3>
              {(() => {
                const totalCount = features.custom_entries?.length || 0
                const activeCount = (features.custom_entries || []).filter((e) => e.enabled !== false).length
                const isBoostActive = isRunning && activeCount > 0
                return (
                  <span
                    className="badge"
                    style={{
                      background: isBoostActive ? 'rgba(168, 85, 247, 0.15)' : 'rgba(255, 255, 255, 0.06)',
                      color: isBoostActive ? '#c084fc' : 'var(--muted)',
                      border: `1px solid ${isBoostActive ? 'rgba(168, 85, 247, 0.4)' : 'var(--border)'}`,
                      fontWeight: 600,
                      fontSize: 11,
                    }}
                  >
                    {!isInstalled
                      ? '🔴 Не установлен'
                      : !isRunning
                      ? '⚪ /boost выключен (Zapret остановлен)'
                      : totalCount === 0
                      ? '⚪ /boost (нет сайтов)'
                      : activeCount === 0
                      ? '⚪ /boost выключен'
                      : `⚡ /boost активен (${activeCount})`}
                  </span>
                )
              })()}
            </div>
            <div className="muted small" style={{ marginTop: 4, lineHeight: 1.45 }}>
              Напишите адрес любого сайта. XKeen автоматически найдёт связанные CDN, картинки и медиа-сервера, подтянет их в карточку другим цветом и направит на максимальной скорости напрямую без расхода VPS.
            </div>
          </div>

          {/* Карточный мастер-свитч /boost */}
          {(() => {
            const totalCount = features.custom_entries?.length || 0
            const activeCount = (features.custom_entries || []).filter((e) => e.enabled !== false).length
            const isBoostChecked = activeCount > 0
            const isBoostPending = pendingKeys.has('custom:all')
            const isBoostDisabled = busy || !isInstalled || totalCount === 0

            return (
              <button
                type="button"
                disabled={isBoostDisabled || isBoostPending}
                onClick={handleToggleAllCustomDomains}
                className={isBoostPending ? 'zapret-glow-pulse-purple' : ''}
                style={{
                  width: 54,
                  height: 30,
                  borderRadius: 18,
                  border: 'none',
                  cursor: isBoostDisabled || isBoostPending ? 'not-allowed' : 'pointer',
                  background: isBoostChecked
                    ? 'linear-gradient(135deg, #a855f7 0%, #7c3aed 100%)'
                    : 'rgba(255, 255, 255, 0.15)',
                  position: 'relative',
                  transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                  padding: 2,
                  flexShrink: 0,
                  boxShadow: isBoostChecked ? '0 0 14px rgba(168, 85, 247, 0.45)' : 'none',
                  opacity: totalCount === 0 ? 0.4 : 1,
                }}
                title={
                  totalCount === 0
                    ? 'Добавьте хотя бы один сайт для включения /boost'
                    : isBoostChecked
                    ? 'Выключить все сайты /boost'
                    : 'Включить все сайты /boost'
                }
              >
                <div
                  style={{
                    width: 26,
                    height: 26,
                    borderRadius: '50%',
                    background: '#fff',
                    transform: isBoostChecked ? 'translateX(24px)' : 'translateX(0)',
                    transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                    boxShadow: '0 1px 4px rgba(0,0,0,0.3)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    fontSize: 11,
                    color: isBoostChecked ? '#7c3aed' : '#888',
                    fontWeight: 'bold',
                  }}
                >
                  {isBoostPending ? renderMicroSpinner('#7c3aed', 13) : isBoostChecked ? '✓' : '✕'}
                </div>
              </button>
            )
          })()}
        </div>

        {/* ПОЛЕ ВВОДА САЙТА */}
        <form
          onSubmit={(e) => {
            e.preventDefault()
            handleAddCustomDomain()
          }}
          style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center', marginBottom: 14 }}
        >
          <input
            type="text"
            className="input"
            value={customDomainInput}
            onChange={(e) => setCustomDomainInput(e.target.value)}
            placeholder="Например: mysku.club, habr.com, speedtest.net, twitch.tv..."
            disabled={addingCustom || !isInstalled}
            style={{ flex: 1, minWidth: 260 }}
          />
          <button
            type="submit"
            className="btn primary"
            disabled={addingCustom || !isInstalled || !customDomainInput.trim()}
            style={{
              background: 'linear-gradient(135deg, #a855f7 0%, #7c3aed 100%)',
              border: 'none',
              boxShadow: '0 2px 10px rgba(168, 85, 247, 0.35)',
              display: 'flex',
              alignItems: 'center',
              gap: 6,
            }}
          >
            {addingCustom ? '⏳ Сканирование CDN…' : '⚡ Добавить & Boost'}
          </button>
        </form>

        {/* БЫСТРЫЕ ПОДСКАЗКИ САЙТОВ ДЛЯ ДОБАВЛЕНИЯ В 1 КЛИК */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap', marginBottom: 18 }}>
          <span className="muted small" style={{ fontSize: 11 }}>Быстрый выбор:</span>
          {['mysku.club', 'habr.com', 'speedtest.net', 'twitch.tv', '4pda.to', 'hdrezka.ag'].map((presetDomain) => {
            const alreadyAdded = (features.custom_entries || []).some(
              (e) => e.domain.toLowerCase() === presetDomain.toLowerCase()
            )
            return (
              <button
                key={presetDomain}
                type="button"
                className="btn sm ghost"
                disabled={addingCustom || alreadyAdded || !isInstalled}
                onClick={() => handleAddCustomDomain(presetDomain)}
                style={{
                  fontSize: 11,
                  padding: '2px 8px',
                  borderRadius: 6,
                  opacity: alreadyAdded ? 0.4 : 1,
                  background: alreadyAdded ? 'rgba(255, 255, 255, 0.03)' : 'rgba(168, 85, 247, 0.08)',
                  borderColor: alreadyAdded ? 'var(--border)' : 'rgba(168, 85, 247, 0.25)',
                  color: alreadyAdded ? 'var(--muted)' : '#c084fc',
                }}
                title={alreadyAdded ? 'Уже добавлен' : `Добавить ${presetDomain} и подтянуть его CDN`}
              >
                {alreadyAdded ? `✓ ${presetDomain}` : `+ ${presetDomain}`}
              </button>
            )
          })}
        </div>

        {/* СПИСОК ДОБАВЛЕННЫХ САЙТОВ С РАЗНОЦВЕТНЫМИ CDN БЕЙДЖАМИ */}
        {(!features.custom_entries || features.custom_entries.length === 0) ? (
          <div
            style={{
              padding: '20px 16px',
              textAlign: 'center',
              borderRadius: 12,
              background: 'rgba(0, 0, 0, 0.2)',
              border: '1px dashed rgba(255, 255, 255, 0.1)',
            }}
          >
            <div style={{ fontSize: 24, marginBottom: 6 }}>🌐</div>
            <div className="muted small">
              Пользовательские сайты пока не добавлены. Введите адрес сайта выше (например, <code>mysku.club</code>) и нажмите <b>«⚡ Добавить & Boost»</b>.
            </div>
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            {features.custom_entries.map((entry) => {
              const isChecked = entry.enabled !== false
              const isBoosting = boostingDomain === entry.domain

              return (
                <div
                  key={entry.domain}
                  style={{
                    padding: '14px 16px',
                    borderRadius: 12,
                    background: isChecked ? 'rgba(168, 85, 247, 0.04)' : 'rgba(255, 255, 255, 0.01)',
                    border: isChecked ? '1px solid rgba(168, 85, 247, 0.3)' : '1px solid var(--border)',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 10,
                    transition: 'all 0.2s ease',
                  }}
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 10 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
                      {/* ОСНОВНОЙ ДОМЕН САЙТА — ЦВЕТ 1 (НЕБЕСНО-ГОЛУБОЙ) */}
                      <span
                        className="badge"
                        style={{
                          fontSize: 13,
                          fontWeight: 700,
                          padding: '5px 12px',
                          borderRadius: 8,
                          background: isChecked ? 'rgba(56, 189, 248, 0.15)' : 'rgba(255, 255, 255, 0.05)',
                          color: isChecked ? '#38bdf8' : 'var(--muted)',
                          border: isChecked ? '1px solid rgba(56, 189, 248, 0.4)' : '1px solid var(--border)',
                          letterSpacing: 0.3,
                        }}
                      >
                        🌐 {entry.domain}
                      </span>

                      <span
                        className="badge"
                        style={{
                          fontSize: 10,
                          background: !isRunning
                            ? 'rgba(255, 255, 255, 0.05)'
                            : isChecked
                            ? 'rgba(34, 197, 94, 0.15)'
                            : 'rgba(255, 255, 255, 0.05)',
                          color: !isRunning
                            ? 'var(--muted)'
                            : isChecked
                            ? '#22c55e'
                            : 'var(--muted)',
                          border: `1px solid ${
                            !isRunning
                              ? 'var(--border)'
                              : isChecked
                              ? 'rgba(34, 197, 94, 0.3)'
                              : 'var(--border)'
                          }`,
                        }}
                      >
                        {!isRunning
                          ? (isChecked ? '⚪ DIRECT (Zapret остановлен)' : '⚪ Выключен')
                          : (isChecked ? '🟢 DIRECT (Zapret)' : '⚪ Выключен')}
                      </span>

                      {entry.cdns && entry.cdns.length > 0 && (
                        <span className="muted small" style={{ fontSize: 11 }}>
                          +{entry.cdns.length} CDN подтянуто
                        </span>
                      )}
                    </div>

                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      {/* КНОПКА RE-BOOST */}
                      <button
                        type="button"
                        className="btn sm ghost"
                        disabled={isBoosting || !isInstalled}
                        onClick={() => handleBoostCustomDomain(entry.domain)}
                        style={{
                          fontSize: 11,
                          padding: '3px 8px',
                          color: '#c084fc',
                          borderColor: 'rgba(168, 85, 247, 0.3)',
                        }}
                        title="Повторно проверить и до-подтянуть новые CDN адреса сайта"
                      >
                        {isBoosting ? '⏳ Boost…' : '⚡ Boost'}
                      </button>

                      {/* КНОПКА УДАЛЕНИЯ */}
                      <button
                        type="button"
                        className="btn sm ghost"
                        disabled={!isInstalled}
                        onClick={() => handleRemoveCustomDomain(entry.domain)}
                        style={{ fontSize: 11, padding: '3px 8px', color: '#ef4444' }}
                        title="Удалить сайт из списка"
                      >
                        🗑️
                      </button>

                      {/* СВИТЧ ВКЛЮЧЕНИЯ/ВЫКЛЮЧЕНИЯ САЙТА ("Блоки, которые можно выключить") */}
                      {(() => {
                        const customPendingKey = `custom:${normalizeDomainInput(entry.domain)}`
                        const isCustomBusy = pendingKeys.has(customPendingKey)
                        return (
                          <button
                            type="button"
                            disabled={!isInstalled || isCustomBusy}
                            onClick={() => handleToggleCustomDomain(entry.domain, isChecked)}
                            className={isCustomBusy ? 'zapret-glow-pulse-purple' : ''}
                            style={{
                              width: 44,
                              height: 24,
                              borderRadius: 14,
                              border: 'none',
                              cursor: !isInstalled || isCustomBusy ? 'not-allowed' : 'pointer',
                              background: isChecked
                                ? 'linear-gradient(135deg, #a855f7 0%, #7c3aed 100%)'
                                : 'rgba(255, 255, 255, 0.15)',
                              position: 'relative',
                              transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                              padding: 2,
                              flexShrink: 0,
                              boxShadow: isChecked ? '0 0 10px rgba(168, 85, 247, 0.4)' : 'none',
                            }}
                            title={isChecked ? 'Выключить обход для этого сайта' : 'Включить обход для этого сайта'}
                          >
                            <div
                              style={{
                                width: 20,
                                height: 20,
                                borderRadius: '50%',
                                background: '#fff',
                                transform: isChecked ? 'translateX(20px)' : 'translateX(0)',
                                transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                                boxShadow: '0 1px 3px rgba(0,0,0,0.3)',
                                display: 'flex',
                                alignItems: 'center',
                                justifyContent: 'center',
                                fontSize: 9,
                                color: isChecked ? '#7c3aed' : '#888',
                                fontWeight: 'bold',
                              }}
                            >
                              {isCustomBusy ? renderMicroSpinner(isChecked ? '#7c3aed' : '#888', 11) : isChecked ? '✓' : '✕'}
                            </div>
                          </button>
                        )
                      })()}
                    </div>
                  </div>

                  {/* ПОДТЯНУТЫЕ CDN ДОМЕНЫ — ЦВЕТ 2 (ФИОЛЕТОВО-РОЗОВЫЙ /BOOST) */}
                  {entry.cdns && entry.cdns.length > 0 ? (() => {
                    const isExpanded = expandedCdnDomains.has(entry.domain)
                    const totalCdns = entry.cdns.length
                    const shouldCollapse = totalCdns > 2
                    return (
                      <div style={{ display: 'flex', flexDirection: 'column', gap: 6, paddingTop: 6, borderTop: '1px solid rgba(255, 255, 255, 0.04)' }}>
                        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexWrap: 'wrap', gap: 6 }}>
                          <span style={{ fontSize: 11, color: isChecked ? '#c084fc' : 'var(--muted)', fontWeight: 600, display: 'flex', alignItems: 'center', gap: 4 }}>
                            ⚡ /boost CDN:
                          </span>
                          {shouldCollapse && (
                            <button
                              type="button"
                              onClick={() => {
                                setExpandedCdnDomains((prev) => {
                                  const next = new Set(prev)
                                  if (next.has(entry.domain)) next.delete(entry.domain)
                                  else next.add(entry.domain)
                                  return next
                                })
                              }}
                              style={{
                                background: 'transparent',
                                border: 'none',
                                color: isChecked ? '#c084fc' : 'var(--muted)',
                                fontSize: 11,
                                cursor: 'pointer',
                                padding: '2px 6px',
                                borderRadius: 4,
                                display: 'inline-flex',
                                alignItems: 'center',
                                gap: 4,
                                fontWeight: 500,
                              }}
                              title={isExpanded ? 'Скрыть список CDN' : 'Показать все связанные CDN'}
                            >
                              ⚡ Связанные CDN ({totalCdns}) [{isExpanded ? 'Скрыть ▲' : 'Показать ▼'}]
                            </button>
                          )}
                        </div>

                        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' }}>
                          {entry.cdns.map((cdn, idx) => {
                            const isHidden = shouldCollapse && !isExpanded && idx >= 2
                            return (
                              <span
                                key={cdn}
                                className="badge"
                                style={{
                                  display: isHidden ? 'none' : 'inline-flex',
                                  fontSize: 10,
                                  fontFamily: 'Consolas, monospace',
                                  padding: '3px 8px',
                                  borderRadius: 6,
                                  background: isChecked
                                    ? 'linear-gradient(135deg, rgba(168, 85, 247, 0.18) 0%, rgba(217, 70, 239, 0.18) 100%)'
                                    : 'rgba(255, 255, 255, 0.03)',
                                  color: isChecked ? '#e879f9' : 'var(--muted)',
                                  border: isChecked ? '1px solid rgba(168, 85, 247, 0.45)' : '1px solid var(--border)',
                                  boxShadow: isChecked ? '0 1px 4px rgba(168, 85, 247, 0.1)' : 'none',
                                }}
                              >
                                ⚡ {cdn}
                              </span>
                            )
                          })}
                          {shouldCollapse && !isExpanded && (
                            <span
                              onClick={() => {
                                setExpandedCdnDomains((prev) => {
                                  const next = new Set(prev)
                                  next.add(entry.domain)
                                  return next
                                })
                              }}
                              style={{
                                fontSize: 10.5,
                                color: isChecked ? '#c084fc' : 'var(--muted)',
                                cursor: 'pointer',
                                padding: '2px 6px',
                                background: 'rgba(168, 85, 247, 0.08)',
                                borderRadius: 4,
                                border: '1px dashed rgba(168, 85, 247, 0.3)',
                              }}
                              title="Нажмите, чтобы показать все CDN"
                            >
                              +{totalCdns - 2} еще...
                            </span>
                          )}
                        </div>
                      </div>
                    )
                  })() : (
                    <div style={{ fontSize: 11, color: 'var(--muted)', display: 'flex', alignItems: 'center', gap: 6, paddingTop: 4 }}>
                      <span>Связанные CDN пока не обнаружены (трафик идёт на основной домен). Нажмите <b>«⚡ Boost»</b> для поиска.</span>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </section>
  )
}