import { useState, useEffect, useCallback } from 'react'
import { apiGet, apiPost } from '../api'

interface MihomoReleaseItem {
  tag_name: string
  name: string
  published_at?: string
  prerelease: boolean
  body: string[]
  is_current: boolean
  download_url?: string
  file_size?: number
}

interface MihomoReleasesResponse {
  current_version: string
  current_full: string
  latest_version: string
  arch: string
  releases: MihomoReleaseItem[]
}

interface Props {
  isOpen: boolean
  onClose: () => void
  notify: (msg: string, isError?: boolean) => void
  onUpdated?: () => void
}

export default function MihomoCoreModal({ isOpen, onClose, notify, onUpdated }: Props) {

  const [data, setData] = useState<MihomoReleasesResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [installingTag, setInstallingTag] = useState<string | null>(null)
  const [stage, setStage] = useState('')
  const [customTag, setCustomTag] = useState('')
  const [showAlpha, setShowAlpha] = useState(false)
  const [expandedNotes, setExpandedNotes] = useState<Record<string, boolean>>({})

  const loadReleases = useCallback(async () => {
    setLoading(true)
    try {
      const res = await apiGet<MihomoReleasesResponse>('mihomo/releases')
      setData(res)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка загрузки релизов Mihomo', true)
    } finally {
      setLoading(false)
    }
  }, [notify])

  useEffect(() => {
    loadReleases()
  }, [loadReleases])

  const handleInstall = async (tag: string, customUrl?: string) => {
    if (!tag) return
    setInstallingTag(tag)
    setStage(`Загрузка архива ядра Mihomo ${tag}…`)

    try {
      setStage(`Распаковка, проверка бинарника и установка ${tag}…`)
      const res = await apiPost<{ success: boolean; version: string; full_version?: string; message: string }>('mihomo/install', {
        tag,
        custom_url: customUrl || undefined,
      })

      setStage(`Ядро Mihomo ${res.version || tag} успешно установлено!`)
      notify(res.message || `Ядро Mihomo обновлено до ${tag}`)
      if (onUpdated) onUpdated()
      await loadReleases()
      setTimeout(() => {
        setInstallingTag(null)
        setStage('')
      }, 1800)
    } catch (e) {
      notify(e instanceof Error ? e.message : 'Ошибка установки ядра Mihomo', true)
      setInstallingTag(null)
      setStage('')
    }
  }

  const currentVer = data?.current_version || ''
  const latestVer = data?.latest_version || ''
  const hasUpdate = Boolean(latestVer && currentVer && latestVer !== currentVer && !currentVer.includes(latestVer))

  const filteredReleases = (data?.releases || []).filter((r) => {
    if (showAlpha) return true
    return !r.prerelease && r.tag_name !== 'Prerelease-Alpha'
  })

  const toggleNotes = (tag: string) => {
    setExpandedNotes((prev) => ({ ...prev, [tag]: !prev[tag] }))
  }

  if (!isOpen) return null

  return (
    <div className="modal-backdrop">
      <div className="modal-card" style={{ maxWidth: 660, width: '94vw', maxHeight: '88vh', display: 'flex', flexDirection: 'column' }}>
        {/* Заголовок */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <div
              style={{
                width: 38,
                height: 38,
                borderRadius: 10,
                background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.2), rgba(37, 99, 235, 0.2))',
                border: '1px solid rgba(56, 189, 248, 0.35)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: '#38bdf8',
                flexShrink: 0,
              }}
            >
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="4" y="4" width="16" height="16" rx="2" />
                <rect x="9" y="9" width="6" height="6" />
                <path d="M9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 14h3M1 9h3M1 14h3" />
              </svg>
            </div>
            <div>
              <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Управление ядром Mihomo</h3>
              <div className="muted small">Релизы MetaCubeX, переключение версий и обновление в 1 клик</div>
            </div>
          </div>
          {!installingTag && (
            <button
              type="button"
              className="btn sm ghost"
              onClick={onClose}
              style={{ fontSize: 16, width: 32, height: 32, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', borderRadius: 8, padding: 0 }}
              title="Закрыть"
            >
              ✕
            </button>
          )}
        </div>

        {/* Карточка текущей версии и архитектуры */}
        <div
          style={{
            background: 'linear-gradient(135deg, rgba(30, 41, 59, 0.5) 0%, rgba(15, 23, 42, 0.6) 100%)',
            border: '1px solid rgba(56, 189, 248, 0.2)',
            borderRadius: 10,
            padding: '14px 16px',
            marginBottom: 14,
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: 12,
            boxShadow: '0 4px 16px rgba(0, 0, 0, 0.2)',
          }}
        >
          <div>
            <div className="muted small" style={{ marginBottom: 4 }}>Текущая версия на роутере</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <span className="upd-dot" style={{ background: '#38bdf8', boxShadow: '0 0 8px #38bdf8' }} />
              <span style={{ fontWeight: 700, fontSize: 16, color: '#f8fafc' }}>
                {currentVer || 'Определяется…'}
              </span>
              {data?.arch && (
                <span className="badge" style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.12)', color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.3)' }}>
                  {data.arch}
                </span>
              )}
              {hasUpdate && (
                <span className="badge" style={{ fontSize: 11, background: 'rgba(56, 189, 248, 0.18)', color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.4)' }}>
                  Доступна {latestVer}
                </span>
              )}
            </div>
          </div>

          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <button
              className="btn sm"
              onClick={loadReleases}
              disabled={loading || Boolean(installingTag)}
              title="Обновить список релизов из GitHub"
            >
              {loading ? '⏳ Загрузка…' : '🔄 Обновить список'}
            </button>
            {hasUpdate && !installingTag && (
              <button
                className="btn primary sm"
                onClick={() => handleInstall(latestVer)}
                title={`Обновить ядро до последней стабильной версии ${latestVer}`}
                style={{
                  background: 'linear-gradient(135deg, #0284c7, #2563eb)',
                  boxShadow: '0 2px 10px rgba(37, 99, 235, 0.4)',
                  borderColor: '#38bdf8',
                  fontWeight: 600,
                }}
              >
                🚀 Обновить до {latestVer}
              </button>
            )}
          </div>
        </div>

        {/* Индикатор прогресса установки */}
        {installingTag && (
          <div
            style={{
              background: 'linear-gradient(135deg, rgba(2, 132, 199, 0.15) 0%, rgba(37, 99, 235, 0.15) 100%)',
              border: '1px solid rgba(56, 189, 248, 0.4)',
              borderRadius: 10,
              padding: '12px 16px',
              marginBottom: 14,
              display: 'flex',
              alignItems: 'center',
              gap: 12,
            }}
          >
            <span className="spin-icon" style={{ fontSize: 18, color: '#38bdf8' }}>⏳</span>
            <div>
              <div style={{ fontWeight: 600, color: '#38bdf8' }}>{stage || 'Установка ядра…'}</div>
              <div className="muted small">Служба XKeen будет автоматически перезапущена с новым бинарником</div>
            </div>
          </div>
        )}

        {/* Фильтры релизов */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10, flexWrap: 'wrap', gap: 8 }}>
          <div style={{ fontWeight: 600, fontSize: 13, color: 'var(--text)' }}>
            Доступные официальные релизы ({filteredReleases.length}):
          </div>
          <label style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 12, cursor: 'pointer' }} className="muted">
            <input
              type="checkbox"
              checked={showAlpha}
              onChange={(e) => setShowAlpha(e.target.checked)}
            />
            Показывать Alpha / Dev сборки
          </label>
        </div>

        {/* Список релизов */}
        <div
          style={{
            flex: 1,
            overflowY: 'auto',
            border: '1px solid var(--border)',
            borderRadius: 10,
            background: 'rgba(0, 0, 0, 0.25)',
            padding: '6px 8px',
            marginBottom: 14,
            maxHeight: 340,
          }}
        >
          {loading && (
            <div style={{ padding: '36px 0', textAlign: 'center' }} className="muted">
              ⏳ Загрузка списка релизов из GitHub MetaCubeX…
            </div>
          )}

          {!loading && filteredReleases.length === 0 && (
            <div style={{ padding: '36px 0', textAlign: 'center' }} className="muted">
              Релизы не найдены. Проверьте интернет-соединение роутера.
            </div>
          )}

          {!loading && filteredReleases.map((rel) => {
            const isCur = rel.is_current || rel.tag_name === currentVer || rel.tag_name === `v${currentVer.replace(/^v/, '')}`
            const isLatest = rel.tag_name === latestVer
            const isBusy = installingTag === rel.tag_name
            const expanded = Boolean(expandedNotes[rel.tag_name])
            const dateStr = rel.published_at ? new Date(rel.published_at).toLocaleDateString() : ''

            return (
              <div
                key={rel.tag_name}
                style={{
                  padding: '10px 12px',
                  borderRadius: 8,
                  marginBottom: 4,
                  border: isCur ? '1px solid rgba(56, 189, 248, 0.35)' : '1px solid transparent',
                  background: isCur ? 'rgba(56, 189, 248, 0.08)' : 'rgba(255, 255, 255, 0.02)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 6,
                  transition: 'background 0.15s ease',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 8 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <span style={{ fontWeight: 700, fontSize: 14, color: isCur ? '#38bdf8' : 'var(--text-bright)' }}>
                      {rel.tag_name}
                    </span>
                    {isCur && (
                      <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.2)', color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.35)', fontSize: 11 }}>
                        ✓ Установлена
                      </span>
                    )}
                    {isLatest && !isCur && (
                      <span className="badge" style={{ background: 'rgba(59, 130, 246, 0.25)', color: '#60a5fa', borderColor: 'rgba(59, 130, 246, 0.4)', fontSize: 11 }}>
                        ★ Последний релиз
                      </span>
                    )}
                    {rel.prerelease && (
                      <span className="badge" style={{ background: 'rgba(234, 179, 8, 0.2)', color: '#eab308', borderColor: 'rgba(234, 179, 8, 0.35)', fontSize: 11 }}>
                        Alpha / Pre-release
                      </span>
                    )}
                    {dateStr && <span className="muted small">({dateStr})</span>}
                  </div>

                  <div style={{ display: 'flex', gap: 6 }}>
                    {rel.body && rel.body.length > 0 && (
                      <button
                        className="btn ghost sm"
                        style={{ fontSize: 11, padding: '3px 8px' }}
                        onClick={() => toggleNotes(rel.tag_name)}
                      >
                        {expanded ? 'Скрыть инфо' : 'Список изменений'}
                      </button>
                    )}
                    <button
                      className={`btn sm ${isCur ? 'ghost' : isLatest ? 'primary' : ''}`}
                      style={{
                        fontSize: 12,
                        padding: '4px 12px',
                        ...(isLatest && !isCur ? { background: 'linear-gradient(135deg, #0284c7, #2563eb)', borderColor: '#38bdf8' } : {})
                      }}
                      disabled={Boolean(installingTag)}
                      onClick={() => handleInstall(rel.tag_name, rel.download_url)}
                    >
                      {isBusy ? '⏳ Установка…' : isCur ? 'Переустановить' : 'Установить'}
                    </button>
                  </div>
                </div>

                {expanded && rel.body && rel.body.length > 0 && (
                  <div style={{ padding: '8px 12px', background: 'rgba(0, 0, 0, 0.35)', border: '1px solid var(--border)', borderRadius: 6, fontSize: 12, marginTop: 4 }}>
                    <ul style={{ margin: 0, paddingLeft: 16 }}>
                      {rel.body.map((b, idx) => (
                        <li key={idx} style={{ marginBottom: 3 }}>{b}</li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            )
          })}
        </div>

        {/* Ручной ввод версии */}
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
          <input
            className="input"
            style={{ flex: 1, padding: '7px 12px', fontSize: 13 }}
            placeholder="Ввести версию вручную (например: v1.19.31)"
            value={customTag}
            onChange={(e) => setCustomTag(e.target.value)}
            disabled={Boolean(installingTag)}
          />
          <button
            className="btn"
            style={{ padding: '7px 16px', fontSize: 13 }}
            disabled={!customTag.trim() || Boolean(installingTag)}
            onClick={() => handleInstall(customTag.trim())}
          >
            Установить версию
          </button>
        </div>
      </div>
    </div>
  )
}
