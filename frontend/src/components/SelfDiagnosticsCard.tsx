import { useCallback, useEffect, useState } from 'react'
import { apiGet, apiPost } from '../api'
import type { SystemDiagnostics } from '../types'

interface Props {
  notify: (msg: string, isError?: boolean) => void
  onSwitchTab?: (tab: 'dashboard' | 'servers' | 'devices' | 'settings' | 'google-ai') => void
  refresh?: () => void
}

export default function SelfDiagnosticsCard({ notify, onSwitchTab, refresh }: Props) {
  const [diag, setDiag] = useState<SystemDiagnostics | null>(null)
  const [loading, setLoading] = useState(false)
  const [lastCheck, setLastCheck] = useState<Date | null>(null)
  const [healing, setHealing] = useState(false)

  const runDiagnostics = useCallback(async () => {
    setLoading(true)
    try {
      const res = await apiGet<SystemDiagnostics>('system/diagnostics')
      setDiag(res)
      setLastCheck(new Date())
    } catch (e: any) {
      console.warn('Ошибка глубокой диагностики:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    runDiagnostics()
  }, [runDiagnostics])

  const handleFixDns = async () => {
    setHealing(true)
    try {
      await apiPost('adguard/sync-firewall')
      notify('Сетевые правила DNS переприменены')
      runDiagnostics()
      refresh?.()
    } catch (e: any) {
      notify('Ошибка восстановления DNS: ' + (e.message || e), true)
    } finally {
      setHealing(false)
    }
  }

  const handleStartMihomo = async () => {
    setHealing(true)
    try {
      await apiPost('service/start')
      notify('Команда запуска ядра Mihomo отправлена')
      setTimeout(runDiagnostics, 2000)
      refresh?.()
    } catch (e: any) {
      notify('Ошибка запуска ядра: ' + (e.message || e), true)
    } finally {
      setHealing(false)
    }
  }

  const isHealthy = diag?.status === 'healthy'
  const isDegraded = diag?.status === 'degraded'
  const isError = diag?.status === 'error'

  const statusColor = isHealthy ? '#10b981' : isDegraded ? '#eab308' : '#ef4444'
  const statusLabel = isHealthy
    ? 'Все системы работают штатно'
    : isDegraded
    ? 'Требуется внимание'
    : isError
    ? 'Обнаружен сбой службы'
    : 'Состояние неизвестно'

  const hasData = Boolean(diag?.components?.adguard && diag?.components?.mihomo && diag?.components?.zapret && diag?.storage && diag?.iptables)

  return (
    <section
      className="card self-diagnostics-card"
      data-testid="self-diagnostics-card"
      style={{
        marginBottom: 16,
        border: `1px solid ${isHealthy ? 'rgba(16, 185, 129, 0.3)' : statusColor}`,
        background: isHealthy
          ? 'linear-gradient(180deg, rgba(16, 185, 129, 0.04) 0%, rgba(255, 255, 255, 0.01) 100%)'
          : 'linear-gradient(180deg, rgba(239, 68, 68, 0.05) 0%, rgba(255, 255, 255, 0.01) 100%)',
      }}
    >
      {/* Шапка карточки */}
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          flexWrap: 'wrap',
          gap: 10,
          marginBottom: 12,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <span style={{ fontSize: 20 }}>🩺</span>
          <div>
            <h2 style={{ margin: 0, fontSize: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
              Самодиагностика в один клик
              <span
                className="badge"
                style={{
                  fontSize: 11,
                  background: isHealthy ? 'rgba(16, 185, 129, 0.15)' : 'rgba(239, 68, 68, 0.15)',
                  color: statusColor,
                  border: `1px solid ${statusColor}`,
                }}
              >
                {isHealthy ? '🟢' : isDegraded ? '🟡' : '🔴'} {statusLabel}
              </span>
            </h2>
            <div className="muted small" style={{ fontSize: 11, marginTop: 2 }}>
              Zero-Touch аудит ядра Mihomo, AdGuard Home, Zapret, iptables и диска /opt
              {lastCheck && ` · обновлено в ${lastCheck.toLocaleTimeString()}`}
            </div>
          </div>
        </div>

        <div style={{ display: 'flex', gap: 8 }}>
          <button
            type="button"
            className="btn ghost sm"
            onClick={runDiagnostics}
            disabled={loading || healing}
            style={{ display: 'flex', alignItems: 'center', gap: 6 }}
            data-testid="run-self-diagnostics-btn"
          >
            {loading ? '⏳ Проверка…' : '⚡ Проверить сейчас'}
          </button>
          {onSwitchTab && (
            <button
              type="button"
              className="btn ghost sm"
              onClick={() => onSwitchTab('settings')}
              title="Открыть системные настройки"
            >
              ⚙️
            </button>
          )}
        </div>
      </div>

      {/* Предупреждения и самолечение (Zero-Touch) */}
      {hasData && diag && (
        <>
          {diag.components.adguard.loop_detected && (
            <div
              style={{
                background: 'rgba(239, 68, 68, 0.1)',
                border: '1px solid #ef4444',
                padding: '8px 12px',
                borderRadius: 6,
                marginBottom: 12,
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: 8,
              }}
            >
              <div style={{ fontSize: 12 }}>
                <b style={{ color: '#ef4444' }}>⚠️ Внимание: Обнаружена циклическая петля DNS!</b>
                <p style={{ margin: '2px 0 0' }} className="muted">
                  AdGuard Home перенаправляет запросы сам на себя. Рекомендуется автоматическое исправление upstream-серверов.
                </p>
              </div>
              <button
                className="btn btn-danger sm"
                onClick={handleFixDns}
                disabled={healing}
              >
                {healing ? '⏳ Лечение…' : '⚡ Устранить петлю'}
              </button>
            </div>
          )}

          {!diag.components.mihomo.running && diag.components.mihomo.installed && (
            <div
              style={{
                background: 'rgba(234, 179, 8, 0.1)',
                border: '1px solid #eab308',
                padding: '8px 12px',
                borderRadius: 6,
                marginBottom: 12,
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                gap: 8,
              }}
            >
              <div style={{ fontSize: 12 }}>
                <b style={{ color: '#eab308' }}>⚠️ Служба Mihomo остановлена</b>
                <p style={{ margin: '2px 0 0' }} className="muted">
                  Маршрутизация трафика приостановлена. Запустите службу для восстановления работы прокси.
                </p>
              </div>
              <button
                className="btn btn-warning sm"
                onClick={handleStartMihomo}
                disabled={healing}
              >
                {healing ? '⏳ Запуск…' : '▶ Запустить ядро'}
              </button>
            </div>
          )}

          {!diag.storage.opt_rw && (
            <div
              style={{
                background: 'rgba(239, 68, 68, 0.1)',
                border: '1px solid #ef4444',
                padding: '8px 12px',
                borderRadius: 6,
                marginBottom: 12,
              }}
            >
              <b style={{ color: '#ef4444', fontSize: 12 }}>
                🔴 Диск /opt смонтирован в режиме только для чтения (Read-Only)
              </b>
              <p style={{ margin: '2px 0 0', fontSize: 11 }} className="muted">
                Файловая система флеш-накопителя заблокирована от записи. Проверьте диск на ошибки через KeeneticOS.
              </p>
            </div>
          )}
        </>
      )}

      {/* Сетка компонентов */}
      {hasData && diag ? (
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
            gap: 10,
          }}
        >
          {/* 1. Mihomo */}
          <div
            style={{
              background: 'rgba(255, 255, 255, 0.03)',
              border: '1px solid var(--border)',
              borderRadius: 6,
              padding: '8px 10px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={{ fontSize: 12, fontWeight: 600 }}>📦 Ядро Mihomo</span>
              <span
                style={{
                  fontSize: 10,
                  color: diag.components.mihomo.running ? '#10b981' : '#ef4444',
                  fontWeight: 700,
                }}
              >
                {diag.components.mihomo.running ? 'В СЕТИ 🟢' : 'ОСТАНОВЛЕНО 🔴'}
              </span>
            </div>
            <div style={{ fontSize: 11, marginTop: 4 }} className="muted">
              Версия: <b style={{ color: 'var(--text)' }}>{diag.components.mihomo.version || 'v1.19+'}</b>
            </div>
            <div style={{ fontSize: 11, marginTop: 2 }} className="muted">
              TUN-интерфейс: <b>{diag.components.mihomo.tun_active ? 'Активен' : 'TProxy/Redir'}</b>
            </div>
          </div>

          {/* 2. AdGuard Home */}
          <div
            style={{
              background: 'rgba(255, 255, 255, 0.03)',
              border: '1px solid var(--border)',
              borderRadius: 6,
              padding: '8px 10px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={{ fontSize: 12, fontWeight: 600 }}>🛡️ AdGuard Home</span>
              <span
                style={{
                  fontSize: 10,
                  color: diag.components.adguard.running ? '#10b981' : 'var(--muted)',
                  fontWeight: 700,
                }}
              >
                {diag.components.adguard.running ? 'АКТИВЕН 🟢' : 'ВЫКЛЮЧЕН ⚪'}
              </span>
            </div>
            <div style={{ fontSize: 11, marginTop: 4 }} className="muted">
              DNS-порт: <b className="mono" style={{ color: 'var(--text)' }}>:{diag.components.adguard.dns_port}</b>
            </div>
            <div style={{ fontSize: 11, marginTop: 2 }} className="muted">
              Перехват 53: <b>{diag.iptables.adguard_chain_active ? 'iptables ON' : 'Выключен'}</b>
            </div>
          </div>

          {/* 3. Zapret */}
          <div
            style={{
              background: 'rgba(255, 255, 255, 0.03)',
              border: '1px solid var(--border)',
              borderRadius: 6,
              padding: '8px 10px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={{ fontSize: 12, fontWeight: 600 }}>⚡ Zapret (DPI)</span>
              <span
                style={{
                  fontSize: 10,
                  color: diag.components.zapret.running ? '#10b981' : 'var(--muted)',
                  fontWeight: 700,
                }}
              >
                {diag.components.zapret.running ? 'В СЕТИ 🟢' : 'ГОТОВ ⚪'}
              </span>
            </div>
            <div style={{ fontSize: 11, marginTop: 4 }} className="muted">
              Движок: <b style={{ color: 'var(--text)' }}>{diag.components.zapret.version || 'v1.0.5'}</b>
            </div>
            <div style={{ fontSize: 11, marginTop: 2 }} className="muted">
              Модули: <b>{diag.components.zapret.binaries_found.join(', ') || 'nfqws2'}</b>
            </div>
          </div>

          {/* 4. Накопитель /opt */}
          <div
            style={{
              background: 'rgba(255, 255, 255, 0.03)',
              border: '1px solid var(--border)',
              borderRadius: 6,
              padding: '8px 10px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={{ fontSize: 12, fontWeight: 600 }}>💾 Накопитель /opt</span>
              <span
                style={{
                  fontSize: 10,
                  color: diag.storage.opt_rw ? '#10b981' : '#ef4444',
                  fontWeight: 700,
                }}
              >
                {diag.storage.opt_rw ? 'RW 🟢' : 'RO 🔴'}
              </span>
            </div>
            <div style={{ fontSize: 11, marginTop: 4 }} className="muted">
              Свободно:{' '}
              <b style={{ color: 'var(--text)' }}>
                {(diag.storage.free_space_mb / 1024).toFixed(1)} ГБ
              </b>{' '}
              из {(diag.storage.total_space_mb / 1024).toFixed(1)} ГБ
            </div>
            <div style={{ fontSize: 11, marginTop: 2 }} className="muted">
              ФС: <b>{diag.storage.opt_mounted ? 'Смонтирован' : 'Не смонтирован'}</b>
            </div>
          </div>
        </div>
      ) : (
        <div style={{ padding: 12, textAlign: 'center' }} className="muted small">
          Загрузка диагностических данных роутера…
        </div>
      )}
    </section>
  )
}
