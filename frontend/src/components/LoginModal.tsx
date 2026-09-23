import { useState } from 'react'
import { apiPost } from '../api'

interface LoginModalProps {
  isOpen?: boolean
  onSuccess: () => void
  notify?: (msg: string, error?: boolean) => void
}

export default function LoginModal({ isOpen = true, onSuccess, notify }: LoginModalProps) {
  if (!isOpen) return null
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleLogin = async (e?: React.FormEvent) => {
    if (e) e.preventDefault()
    if (!password) {
      setError('Введите пароль')
      return
    }

    setLoading(true)
    setError(null)
    try {
      const res = await apiPost<{ authenticated: boolean; token?: string }>('auth/login', { password })
      if (res.authenticated) {
        notify?.('Авторизация успешна')
        onSuccess()
      } else {
        setError('Неверный пароль')
      }
    } catch (err: any) {
      setError(err?.message || 'Ошибка входа')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="modal-backdrop login-backdrop">
      <div className="modal-card login-card">
        <div className="login-header">
          <div className="login-icon">🔒</div>
          <h2>Авторизация</h2>
          <p className="muted">Панель управления XKeen Route защищена паролем</p>
        </div>

        <form onSubmit={handleLogin} className="login-form">
          <div className="form-group">
            <label>Пароль администратора:</label>
            <input
              type="password"
              className="input-text login-input"
              placeholder="••••••••"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoFocus
              disabled={loading}
            />
          </div>

          {error && <div className="login-error">⚠️ {error}</div>}

          <div className="login-actions">
            <button type="submit" className="btn btn-primary login-btn" disabled={loading}>
              {loading ? 'Проверка…' : '🔑 Войти в панель'}
            </button>
          </div>
        </form>

        <div className="login-footer muted">
          <small>Для сброса пароля выполните через SSH: <code>xkeen-route reset-password</code></small>
        </div>
      </div>
    </div>
  )
}
