import React from 'react'
import type { VkTurnStatus } from '../../types'
import { installProgressText, type VkTurnAuthMode } from './vkturnHelpers'

export interface VkTurnSetupWizardProps {
  status: VkTurnStatus | null
  selectedSource: VkTurnAuthMode
  setSourceMode: (mode: VkTurnAuthMode) => void
  peerHost: string
  handleHostChange: (val: string) => void
  portInput: string
  setPortInput: React.Dispatch<React.SetStateAction<string>>
  password: string
  setPassword: React.Dispatch<React.SetStateAction<string>>
  wizardTouchedRef: React.MutableRefObject<boolean>
  callLinks: string[]
  setCallLinks: React.Dispatch<React.SetStateAction<string[]>>
  handleCallLinkChange: (idx: number, val: string) => void
  handleImportFromClipboard: () => void
  handleWizardEnable: () => void
  actionPending: boolean
  installing: boolean
  maxLinks: number
  profileToken: string | null
  renderVkAccountCard: () => React.ReactNode
  renderAuthModal: () => React.ReactNode
}

export function VkTurnSetupWizard({
  status,
  selectedSource,
  setSourceMode,
  peerHost,
  handleHostChange,
  portInput,
  setPortInput,
  password,
  setPassword,
  wizardTouchedRef,
  callLinks,
  setCallLinks,
  handleCallLinkChange,
  handleImportFromClipboard,
  handleWizardEnable,
  actionPending,
  installing,
  maxLinks,
  profileToken,
  renderVkAccountCard,
  renderAuthModal,
}: VkTurnSetupWizardProps) {
  return (
  <div className="vkturn-container" style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
    <section className="vk-section" style={{ padding: '20px 24px' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16, flexWrap: 'wrap', marginBottom: 18 }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 4 }}>
            <h2 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Мастер настройки VK-туннеля</h2>
            <span className="badge badge-warning" style={{ fontSize: 12 }}>Первоначальная настройка</span>
          </div>
          <p className="muted" style={{ margin: 0, fontSize: 13.5 }}>
            Трафик идёт через серверы звонков ВКонтакте до вашего сервера csqtt на VPS — работает даже при белых списках.
          </p>
        </div>
        <button
          type="button"
          className="btn btn-secondary"
          onClick={handleImportFromClipboard}
          style={{ fontSize: 12.5, padding: '6px 12px', display: 'inline-flex', alignItems: 'center', gap: 6 }}
          title="Импортировать ссылку csqtt:// из буфера обмена"
        >
          📋 Вставить ссылку csqtt://
        </button>
      </div>

      <h3 style={{ margin: '0 0 12px 0', fontSize: 15 }}>1️⃣ Сервер csqtt на VPS</h3>
      <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap', marginBottom: 16 }}>
        <div style={{ flex: '1 1 260px', maxWidth: 420 }}>
          <label style={{ display: 'block', fontSize: 13, marginBottom: 6, fontWeight: 600 }}>
            Адрес сервера (IP, домен или ссылка csqtt://):
          </label>
          <input
            type="text"
            className="input"
            placeholder="203.0.113.10 или csqtt://connect?..."
            value={peerHost}
            onChange={(e) => handleHostChange(e.target.value)}
            style={{ width: '100%' }}
          />
        </div>
        <div style={{ flex: '0 1 160px' }}>
          <label style={{ display: 'block', fontSize: 13, marginBottom: 6, fontWeight: 600 }}>
            Порт:
          </label>
          <input
            type="number"
            className="input"
            data-testid="vk-port-input"
            aria-label="Порт сервера csqtt"
            min={1}
            max={65535}
            value={portInput}
            onChange={(e) => {
              wizardTouchedRef.current = true
              setPortInput(e.target.value)
            }}
            style={{ width: '100%' }}
          />
        </div>
      </div>

      <h3 style={{ margin: '20px 0 12px 0', fontSize: 15 }}>2️⃣ Пароль подключения</h3>
      <div style={{ marginBottom: 8, maxWidth: 420 }}>
        <input
          type="password"
          className="input"
          autoComplete="new-password"
          placeholder={status?.has_password ? 'Оставьте пустым, чтобы сохранить текущий пароль' : 'Пароль от сервера csqtt'}
          value={password}
          onChange={(e) => {
            wizardTouchedRef.current = true
            setPassword(e.target.value)
          }}
          style={{ width: '100%' }}
        />
      </div>
      <p className="muted" style={{ fontSize: 13, marginTop: 0, marginBottom: 0 }}>
        {status?.has_password
          ? '🔑 Пароль уже сохранён на роутере — можно оставить поле пустым или ввести новый.'
          : '🔑 Это пароль туннеля, заданный при установке сервера csqtt (или выданный владельцем сервера). Адрес внутри туннеля сервер выдаст сам.'}
      </p>

      <h3 style={{ margin: '20px 0 12px 0', fontSize: 15 }}>3️⃣ Авторизация ВКонтакте и звонки</h3>
      {renderVkAccountCard()}

      {(status?.has_token || profileToken || status?.has_profile_token) && (
        <div className="vk-source-pills" role="group" aria-label="Источник звонков" style={{ marginTop: 12 }}>
          {status?.has_token && (
            <label className={`vk-source-pill${selectedSource === 'oauth' ? ' active' : ''}`}>
              <input
                type="radio"
                name="vkturn-source"
                checked={selectedSource === 'oauth'}
                onChange={() => setSourceMode('oauth')}
                style={{ marginRight: 6 }}
              />
              Мой аккаунт VK
            </label>
          )}
          {(profileToken || (profileToken === null && status?.has_profile_token)) && (
            <label className={`vk-source-pill${selectedSource === 'profile_token' ? ' active' : ''}`}>
              <input
                type="radio"
                name="vkturn-source"
                checked={selectedSource === 'profile_token'}
                onChange={() => setSourceMode('profile_token')}
                style={{ marginRight: 6 }}
              />
              Токен профиля
            </label>
          )}
          <label className={`vk-source-pill${selectedSource === 'manual' ? ' active' : ''}`}>
            <input
              type="radio"
              name="vkturn-source"
              checked={selectedSource === 'manual'}
              onChange={() => setSourceMode('manual')}
              style={{ marginRight: 6 }}
            />
            Ссылки на звонки
          </label>
        </div>
      )}

      {selectedSource === 'manual' && (
        <>
          <p className="muted" style={{ fontSize: 13, marginTop: 12, marginBottom: 12 }}>
            💡 Или укажите прямые ссылки на звонки ВК вручную (до {maxLinks} ссылок):
          </p>

          <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginBottom: 14 }}>
            {callLinks.map((link, idx) => (
              <div key={idx} style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <input
                  type="text"
                  className="input"
                  placeholder={idx === 0 ? 'Основная ссылка на звонок (https://vk.com/call/join/...)' : 'Запасная ссылка'}
                  value={link}
                  onChange={(e) => handleCallLinkChange(idx, e.target.value)}
                  style={{ flex: 1 }}
                />
                {callLinks.length > 1 && (
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => setCallLinks((prev) => prev.filter((_, i) => i !== idx))}
                    title="Удалить ссылку"
                    style={{ padding: '6px 10px' }}
                  >
                    ✕
                  </button>
                )}
              </div>
            ))}
            {callLinks.length < maxLinks && (
              <div>
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setCallLinks((prev) => [...prev, ''])}
                  style={{ fontSize: 13 }}
                >
                  + Добавить запасную ссылку
                </button>
              </div>
            )}
          </div>
        </>
      )}

      <div style={{ marginTop: 20, paddingTop: 16, borderTop: '1px solid var(--border)' }}>
        <button
          type="button"
          className="btn btn-primary"
          disabled={actionPending}
          onClick={handleWizardEnable}
          style={{ padding: '10px 24px', fontSize: 14, fontWeight: 600 }}
        >
          {actionPending ? (installing ? '⏳ Скачиваю компонент…' : '⏳ Активация…') : 'Включить'}
        </button>
        {installing && (
          <div className="muted" style={{ marginTop: 10, fontSize: 13 }} data-testid="install-progress">
            ⏳ Скачиваю компонент VK-туннеля: {installProgressText(status?.install)}
          </div>
        )}
      </div>
    </section>
    {renderAuthModal()}
  </div>
  )
}
