import type { Dispatch, SetStateAction } from 'react'
import type { ZapretFeatures } from '../../types'
import { renderMicroSpinner } from './zapretHelpers'

interface CommunityHostlistsSectionProps {
  busy: boolean
  communityUrl: string
  setCommunityUrl: Dispatch<SetStateAction<string>>
  syncingCommunity: boolean
  pendingKeys: Set<string>
  features: ZapretFeatures
  isRunning: boolean
  isInstalled: boolean
  handleSyncCommunityHostlist: () => void
  handleSaveCommunityHostlistUrl: (newUrl: string) => void
  handleToggleFeature: (key: keyof ZapretFeatures) => void
}

export function CommunityHostlistsSection({
  busy,
  communityUrl,
  setCommunityUrl,
  syncingCommunity,
  pendingKeys,
  features,
  isRunning,
  isInstalled,
  handleSyncCommunityHostlist,
  handleSaveCommunityHostlistUrl,
  handleToggleFeature,
}: CommunityHostlistsSectionProps) {
  return (
      <section
        className="card"
        style={{
          padding: '22px 24px',
          background: 'linear-gradient(135deg, rgba(59, 130, 246, 0.05) 0%, rgba(15, 23, 42, 0.5) 100%)',
          border: '1px solid rgba(59, 130, 246, 0.3)',
          borderRadius: 16,
          boxShadow: '0 8px 24px rgba(59, 130, 246, 0.05)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start', flexWrap: 'wrap', gap: 14, marginBottom: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <div
              style={{
                width: 40,
                height: 40,
                borderRadius: 10,
                background: 'rgba(59, 130, 246, 0.15)',
                border: '1px solid rgba(59, 130, 246, 0.35)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 20,
              }}
            >
              🌐
            </div>
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                <h3 style={{ margin: 0, fontSize: 17, fontWeight: 700 }}>
                  Автообновление списков (Community Hostlists)
                </h3>
                <span
                  className="badge"
                  style={{
                    background: features.community_hostlist_enabled ? (isRunning ? 'rgba(34, 197, 94, 0.15)' : 'rgba(255, 255, 255, 0.06)') : 'rgba(255, 255, 255, 0.06)',
                    color: features.community_hostlist_enabled ? (isRunning ? '#4ade80' : 'var(--muted)') : 'var(--muted)',
                    border: `1px solid ${features.community_hostlist_enabled ? (isRunning ? 'rgba(34, 197, 94, 0.3)' : 'var(--border)') : 'var(--border)'}`,
                    fontSize: 11,
                  }}
                >
                  {features.community_hostlist_enabled ? (isRunning ? '🟢 АКТИВЕН' : '⚪ ВЫКЛЮЧЕН (Zapret остановлен)') : '⚪ ВЫКЛЮЧЕН'}
                </span>
                {features.community_hostlist_count ? (
                  <span className="badge" style={{ background: 'rgba(59, 130, 246, 0.15)', color: '#60a5fa', border: '1px solid rgba(59, 130, 246, 0.3)', fontSize: 11 }}>
                    {features.community_hostlist_count.toLocaleString()} доменов
                  </span>
                ) : null}
              </div>
              <div className="muted small" style={{ marginTop: 3 }}>
                Поддержка внешних списков доменов сообщества (Antizapret / Custom URL), автоматическое фоновое обновление и моментальное применение через fast SIGHUP (<code>reload-hosts</code>) без перезапуска nfqws2.
              </div>
            </div>
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            {/* Свитч включения внешнего списка */}
            <button
              type="button"
              disabled={busy || !isInstalled || pendingKeys.has('community_hostlist_enabled')}
              onClick={() => handleToggleFeature('community_hostlist_enabled')}
              style={{
                width: 50,
                height: 28,
                borderRadius: 16,
                border: 'none',
                cursor: busy || !isInstalled ? 'not-allowed' : 'pointer',
                background: features.community_hostlist_enabled
                  ? 'linear-gradient(135deg, #3b82f6 0%, #1d4ed8 100%)'
                  : 'rgba(255, 255, 255, 0.15)',
                position: 'relative',
                transition: 'all 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                padding: 2,
                flexShrink: 0,
                boxShadow: features.community_hostlist_enabled ? '0 0 12px rgba(59, 130, 246, 0.4)' : 'none',
              }}
              title={features.community_hostlist_enabled ? 'Выключить внешний список' : 'Включить внешний список'}
            >
              <div
                style={{
                  width: 24,
                  height: 24,
                  borderRadius: '50%',
                  background: '#fff',
                  transform: features.community_hostlist_enabled ? 'translateX(22px)' : 'translateX(0)',
                  transition: 'transform 0.25s cubic-bezier(0.4, 0, 0.2, 1)',
                  boxShadow: '0 1px 4px rgba(0,0,0,0.3)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  fontSize: 10,
                  color: features.community_hostlist_enabled ? '#1d4ed8' : '#888',
                  fontWeight: 'bold',
                }}
              >
                {pendingKeys.has('community_hostlist_enabled') ? renderMicroSpinner('#1d4ed8', 13) : features.community_hostlist_enabled ? '✓' : '✕'}
              </div>
            </button>
          </div>
        </div>

        {/* НАСТРОЙКИ URL И КНОПКА СИНХРОНИЗАЦИИ */}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
            <input
              type="url"
              className="input"
              value={communityUrl}
              onChange={(e) => setCommunityUrl(e.target.value)}
              onBlur={() => handleSaveCommunityHostlistUrl(communityUrl)}
              placeholder="https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt"
              disabled={syncingCommunity || !isInstalled}
              style={{ flex: 1, minWidth: 280, fontFamily: 'Consolas, monospace', fontSize: 12 }}
            />

            <button
              type="button"
              className="btn primary"
              disabled={syncingCommunity || busy || !isInstalled}
              onClick={handleSyncCommunityHostlist}
              style={{
                background: 'linear-gradient(135deg, #3b82f6 0%, #2563eb 100%)',
                border: 'none',
                boxShadow: '0 2px 10px rgba(59, 130, 246, 0.35)',
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                fontWeight: 600,
              }}
            >
              {syncingCommunity ? (
                <>
                  <span className="spinner sm" />
                  <span>Синхронизация…</span>
                </>
              ) : (
                <>
                  <span>🔄</span>
                  <span>Синхронизировать сейчас</span>
                </>
              )}
            </button>
          </div>

          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 10, fontSize: 12 }}>
            <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer', userSelect: 'none' }}>
              <input
                type="checkbox"
                checked={!!features.community_hostlist_auto_update}
                onChange={() => handleToggleFeature('community_hostlist_auto_update')}
                disabled={busy || !isInstalled}
              />
              <span style={{ color: features.community_hostlist_auto_update ? '#60a5fa' : 'var(--muted)' }}>
                Автоматическое обновление списка каждые 24 часа
              </span>
            </label>

            {features.community_hostlist_last_updated ? (
              <span className="muted small" style={{ fontFamily: 'Consolas, monospace' }}>
                Последняя синхронизация: {features.community_hostlist_last_updated}
              </span>
            ) : null}
          </div>
        </div>
      </section>
  )
}