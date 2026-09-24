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
  if (!isOpen) return null

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

  return (
    <div className="modal-backdrop">
      <div className="modal-card" style={{ maxWidth: 640, width: '94vw', maxHeight: '88vh', display: 'flex', flexDirection: 'column' }}>
        {/* Заголовок */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 14 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 24 }}>⚙️</span>
            <div>
              <h3 style={{ margin: 0, fontSize: 18 }}>Управление ядром Mihomo</h3>
              <div className="muted small">Релизы MetaCubeX, переключение версий и обновление в 1 клик</div>
            </div>
          </div>
          {!installingTag && (
            <button
              type="button"
              className="btn sm ghost"
              onClick={onClose}
              style={{ fontSize: 16, padding: '2px 8px', borderRadius: 6 }}
              title="Закрыть"
            >
              ✕
            </button>
          )}
        </div>

        {/* Карточка текущей версии и архитектуры */}
        <div
          style={{
            background: 'var(--panel-2, rgba(255, 255, 255, 0.04))',
            border: '1px solid var(--border)',
            borderRadius: 8,
            padding: '12px 16px',
            marginBottom: 14,
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: 12,
          }}
        >
          <div>
            <div className="muted small">Текущая версия на роутере</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 2 }}>
              <span style={{ fontWeight: 700, fontSize: 16, color: '#22c55e' }}>
                🟢 {currentVer || 'Определяется…'}
              </span>
              {data?.arch && (
                <span className="badge" style={{ fontSize: 11, background: 'rgba(59, 130, 246, 0.15)', color: '#60a5fa' }}>
                  {data.arch}
                </span>
              )}
            </div>
          </div>

          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
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
              background: 'rgba(34, 197, 94, 0.12)',
              border: '1px solid rgba(34, 197, 94, 0.35)',
              borderRadius: 8,
              padding: '12px 16px',
              marginBottom: 14,
              display: 'flex',
              alignItems: 'center',
              gap: 12,
            }}
          >
            <span className="spin-icon" style={{ fontSize: 18 }}>⏳</span>
            <div>
              <div style={{ fontWeight: 600, color: '#22c55e' }}>{stage || 'Установка ядра…'}</div>
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
            borderRadius: 8,
            background: 'rgba(0, 0, 0, 0.2)',
            padding: '6px 8px',
            marginBottom: 14,
            maxHeight: 340,
          }}
        >
          {loading && (
            <div style={{ padding: '30px 0', textAlign: 'center' }} className="muted">
              ⏳ Загрузка списка релизов из GitHub MetaCubeX…
            </div>
          )}

          {!loading && filteredReleases.length === 0 && (
            <div style={{ padding: '30px 0', textAlign: 'center' }} className="muted">
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
                  borderBottom: '1px solid var(--border)',
                  background: isCur ? 'rgba(34, 197, 94, 0.05)' : 'transparent',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 6,
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 8 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                    <span style={{ fontWeight: 700, fontSize: 14, color: isCur ? '#22c55e' : 'var(--text)' }}>
                      {rel.tag_name}
                    </span>
                    {isCur && (
                      <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.2)', color: '#22c55e', fontSize: 11 }}>
                        ✓ Установлена
                      </span>
                    )}
                    {isLatest && !isCur && (
                      <span className="badge" style={{ background: 'rgba(59, 130, 246, 0.2)', color: '#60a5fa', fontSize: 11 }}>
                        ★ Последний релиз
                      </span>
                    )}
                    {rel.prerelease && (
                      <span className="badge" style={{ background: 'rgba(234, 179, 8, 0.2)', color: '#eab308', fontSize: 11 }}>
                        Alpha / Pre-release
                      </span>
                    )}
                    {dateStr && <span className="muted small">({dateStr})</span>}
                  </div>

                  <div style={{ display: 'flex', gap: 6 }}>
                    {rel.body && rel.body.length > 0 && (
                      <button
                        className="btn ghost sm"
                        style={{ fontSize: 11, padding: '2px 8px' }}
                        onClick={() => toggleNotes(rel.tag_name)}
                      >
                        {expanded ? 'Скрыть инфо' : 'Список изменений'}
                      </button>
                    )}
                    <button
                      className={`btn sm ${isCur ? 'ghost' : isLatest ? 'primary' : ''}`}
                      style={{ fontSize: 12, padding: '4px 10px' }}
                      disabled={Boolean(installingTag)}
                      onClick={() => handleInstall(rel.tag_name, rel.download_url)}
                    >
                      {isBusy ? '⏳ Установка…' : isCur ? 'Переустановить' : 'Установить'}
                    </button>
                  </div>
                </div>

                {expanded && rel.body && rel.body.length > 0 && (
                  <div style={{ padding: '6px 10px', background: 'rgba(0,0,0,0.3)', borderRadius: 6, fontSize: 12, marginTop: 4 }}>
                    <ul style={{ margin: 0, paddingLeft: 16 }}>
                      {rel.body.map((b, idx) => (
                        <li key={idx} style={{ marginBottom: 2 }}>{b}</li>
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
            style={{ flex: 1, padding: '6px 10px', fontSize: 13 }}
            placeholder="Ввести версию вручную (например: v1.19.31)"
            value={customTag}
            onChange={(e) => setCustomTag(e.target.value)}
            disabled={Boolean(installingTag)}
          />
          <button
            className="btn"
            style={{ padding: '6px 14px', fontSize: 13 }}
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
