import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost } from '../../api'
import type { SwapStatus, SwapSizePresetMb } from '../../types'

export function SwapManager({ notify }: { notify: (msg: string, isErr?: boolean) => void }) {
  const [status, setStatus] = useState<SwapStatus | null>(null)
  const [loading, setLoading] = useState(false)
  const [selectedSize, setSelectedSize] = useState<SwapSizePresetMb>(512)

  const loadStatus = useCallback(async () => {
    try {
      const data = await apiGet<SwapStatus>('system/swap/status')
      setStatus(data)
    } catch (e: any) {
      console.warn('Ошибка получения статуса Swap:', e)
    }
  }, [])

  useEffect(() => {
    loadStatus()
  }, [loadStatus])

  const handleCreate = async () => {
    setLoading(true)
    notify(`⏳ Выделение файла подкачки ${selectedSize} МБ на USB... Подождите 10-20 сек.`)
    try {
      const res = await apiPost<SwapStatus>('system/swap/create', { size_mb: selectedSize })
      setStatus(res)
      notify(`✓ Файл подкачки ${selectedSize} МБ успешно создан и активирован!`)
    } catch (err: any) {
      notify(err.message || 'Ошибка создания Swap', true)
    } finally {
      setLoading(false)
    }
  }

  const handleRemove = async () => {
    if (typeof window !== 'undefined' && !window.confirm('Отключить и удалить файл подкачки?')) return
    setLoading(true)
    try {
      const res = await apiPost<SwapStatus>('system/swap/remove', {})
      setStatus(res)
      notify('Файл подкачки отключен и удален')
    } catch (err: any) {
      notify(err.message || 'Ошибка отключения Swap', true)
    } finally {
      setLoading(false)
    }
  }

  if (!status || !status.swap_supported) return null

  const ramUsedMb = Math.max(0, status.ram_total_mb - status.ram_free_mb)
  const ramPercent = Math.min(100, Math.round((ramUsedMb / (status.ram_total_mb || 1)) * 100))
  const swapPercent =
    status.swap_total_mb > 0
      ? Math.min(100, Math.round((status.swap_used_mb / status.swap_total_mb) * 100))
      : 0

  return (
    <div className="card" style={{ marginTop: 16 }} data-testid="swap-manager-card">
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h3 style={{ margin: 0 }}>🚀 Память и файл подкачки (Swap)</h3>
        <span
          className={`badge ${status.swap_active ? 'badge-success' : 'badge-muted'}`}
          data-testid="swap-status-badge"
        >
          {status.swap_active ? '🟢 Swap активен' : '⚪ Swap выключен'}
        </span>
      </div>
      <p className="muted small" style={{ margin: '8px 0 16px' }}>
        Технология защиты от вылетов ядра и нехватки ОЗУ на роутере Keenetic.
        Используется щадящий режим флешки (swappiness=10).
      </p>

      {/* Метрики памяти */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(240px, 1fr))', gap: 12, marginBottom: 16 }}>
        <div style={{ background: 'rgba(255,255,255,0.03)', padding: 12, borderRadius: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13, marginBottom: 6 }}>
            <span>ОЗУ роутера</span>
            <b>{ramUsedMb} / {status.ram_total_mb} МБ ({ramPercent}%)</b>
          </div>
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{
                width: `${ramPercent}%`,
                background: ramPercent > 85 ? '#ef4444' : '#38bdf8',
              }}
            />
          </div>
          <div className="muted small" style={{ marginTop: 4 }}>
            Свободно: {status.ram_free_mb} МБ
          </div>
        </div>

        <div style={{ background: 'rgba(255,255,255,0.03)', padding: 12, borderRadius: 8 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 13, marginBottom: 6 }}>
            <span>Файл подкачки (Swap)</span>
            <b>{status.swap_used_mb} / {status.swap_total_mb} МБ ({swapPercent}%)</b>
          </div>
          <div className="progress-bar">
            <div
              className="progress-fill"
              style={{ width: `${swapPercent}%`, background: '#22c55e' }}
            />
          </div>
          <div className="muted small" style={{ marginTop: 4 }}>
            Накопитель /opt: свободно {status.opt_disk_free_mb} МБ
          </div>
        </div>
      </div>

      {/* Управление */}
      {!status.swap_active ? (
        <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
          <div style={{ display: 'flex', background: 'rgba(255,255,255,0.05)', borderRadius: 6, padding: 3 }}>
            {([256, 512, 1024] as const).map((sz) => (
              <button
                key={sz}
                type="button"
                className={`btn-tab ${selectedSize === sz ? 'active' : ''}`}
                style={{ padding: '6px 14px', fontSize: 12 }}
                onClick={() => setSelectedSize(sz)}
                data-testid={`swap-preset-${sz}`}
              >
                {sz} МБ {sz === 512 ? '★' : ''}
              </button>
            ))}
          </div>
          <button
            className="btn btn-primary"
            disabled={loading}
            onClick={handleCreate}
            style={{ minWidth: 200 }}
            data-testid="swap-create-button"
          >
            {loading ? '⏳ Создание (15 сек)...' : `🚀 Создать Swap ${selectedSize} МБ в 1 клик`}
          </button>
        </div>
      ) : (
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <span className="small muted">
            Файл <code>{status.swap_file_path}</code> ({status.swap_total_mb} МБ) активен. Автозапуск настроен в <code>S01swap</code>.
          </span>
          <button
            className="btn btn-danger"
            disabled={loading}
            onClick={handleRemove}
            data-testid="swap-remove-button"
          >
            {loading ? '⏳ Удаление...' : '🗑 Отключить и удалить Swap'}
          </button>
        </div>
      )}
    </div>
  )
}

export function fmtTrafficBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export function cleanServerDisplayName(name: string): string {
  if (!name) return ''
  return name.trim()
}

export function formatActiveServerStatus(statusText?: string, activeServer?: any, chain?: string[]): string {
  if (chain && chain.length > 0 && activeServer?.id) {
    const idx = chain.indexOf(activeServer.id)
    if (idx === 0) {
      return 'Штатно: активен основной узел #1'
    } else if (idx > 0) {
      return `Резерв #${idx + 1}: основной узел недоступен`
    }
  }
  if (!statusText) {
    return 'Штатный режим: переключения не требуются'
  }
  let s = statusText.replace(/\.?\s*В карантине.*$/i, '').trim()
  s = cleanServerDisplayName(s)
  if (s.length === 0) {
    if (activeServer?.name) {
      return `Штатно: активен узел '${cleanServerDisplayName(activeServer.name)}'`
    }
    return 'Штатный режим: переключения не требуются'
  }
  return s
}