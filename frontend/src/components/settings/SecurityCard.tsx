import { apiPost } from '../../api'
import type { AppSettings } from '../../types'

interface SecurityCardProps {
  settings: AppSettings
  authEnabled: boolean
  setAuthEnabled: (val: boolean) => void
  currentPassword: string
  setCurrentPassword: (val: string) => void
  newPassword: string
  setNewPassword: (val: string) => void
  confirmPassword: string
  setConfirmPassword: (val: string) => void
  authSaving: boolean
  setAuthSaving: (val: boolean) => void
  handleSaveAuth: () => void
  notify: (msg: string, isError?: boolean) => void
  refresh?: () => void
}

export function SecurityCard({
  settings,
  authEnabled,
  setAuthEnabled,
  currentPassword,
  setCurrentPassword,
  newPassword,
  setNewPassword,
  confirmPassword,
  setConfirmPassword,
  authSaving,
  setAuthSaving,
  handleSaveAuth,
  notify,
  refresh,
}: SecurityCardProps) {
  // БЕЗОПАСНОСТЬ И ПАРОЛЬ (Сжатая Apple-карточка)
  return (
      <section className="card" style={{ padding: authEnabled ? '16px' : '8px 16px', transition: 'all 0.25s ease' }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', minHeight: authEnabled ? 'auto' : 36 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 16 }}>🔐</span>
            <div>
              <b style={{ fontSize: 13.5 }}>Безопасность и пароль</b>
              {!authEnabled && (
                <span className="muted small" style={{ marginLeft: 8, fontSize: 12 }}>
                  (пароль выключен — свободный вход)
                </span>
              )}
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span className={`badge ${authEnabled ? 'badge-online' : ''}`} style={{ fontSize: 11, padding: '2px 8px' }}>
              {authEnabled ? '🟢 защита включена' : '⚪ без пароля'}
            </span>
            <label className="switch" style={{ margin: 0 }}>
              <input
                type="checkbox"
                checked={authEnabled}
                onChange={async (e) => {
                  const checked = e.target.checked
                  setAuthEnabled(checked)
                  if (!checked && settings?.auth?.enabled) {
                    if (window.confirm('Отключить защиту паролем для веб-панели?')) {
                      setAuthSaving(true)
                      try {
                        await apiPost('auth/change-password', {
                          enabled: false,
                          new_password: '',
                        })
                        notify('Авторизация отключена')
                        refresh?.()
                      } catch (err: any) {
                        notify(err.message || 'Ошибка отключения пароля', true)
                        setAuthEnabled(true)
                      } finally {
                        setAuthSaving(false)
                      }
                    } else {
                      setAuthEnabled(true)
                    }
                  }
                }}
              />
              <span className="slider" />
            </label>
          </div>
        </div>

        {authEnabled && (
          <div style={{ marginTop: 12, borderTop: '1px solid rgba(255,255,255,0.06)', paddingTop: 12 }}>
            <p className="muted small" style={{ margin: '0 0 10px' }}>Защита веб-панели паролем с сессией (24 часа).</p>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {settings?.auth?.enabled && (
                <label className="row">
                  <span>Текущий пароль</span>
                  <input
                    className="input"
                    type="password"
                    placeholder="Текущий пароль (для подтверждения)"
                    value={currentPassword}
                    onChange={(e) => setCurrentPassword(e.target.value)}
                  />
                </label>
              )}
              <label className="row">
                <span>Новый пароль</span>
                <input
                  className="input"
                  type="password"
                  placeholder="Введите новый пароль"
                  value={newPassword}
                  onChange={(e) => setNewPassword(e.target.value)}
                />
              </label>
              <label className="row">
                <span>Повторите пароль</span>
                <input
                  className="input"
                  type="password"
                  placeholder="Повторите новый пароль"
                  value={confirmPassword}
                  onChange={(e) => setConfirmPassword(e.target.value)}
                />
              </label>
            </div>

            <button className="btn primary" style={{ marginTop: 12 }} onClick={handleSaveAuth} disabled={authSaving}>
              {authSaving ? 'Сохранение…' : '💾 Сохранить настройки доступа'}
            </button>
            <p className="muted small" style={{ marginTop: 8, marginBottom: 0 }}>
              💡 Сброс пароля при утере через SSH: <code style={{ color: 'var(--accent)' }}>xkeen-route reset-password</code>
            </p>
          </div>
        )}
      </section>
  )
}