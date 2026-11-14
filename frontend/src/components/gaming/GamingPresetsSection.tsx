import type { GamingConfig, GamingMode } from '../../types'

export interface GamingPresetsSectionProps {
  cfg: GamingConfig
  toggling: boolean
  handleModeChange: (mode: GamingMode) => void
  handleToggle: (enabled: boolean, overrideMode?: GamingMode) => void
}

export function GamingPresetsSection({
  cfg,
  toggling,
  handleModeChange,
  handleToggle,
}: GamingPresetsSectionProps) {
  return (
  <div className="card" style={{ padding: '16px 18px' }}>
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
      <div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 20 }}>📋</span>
          <h3 style={{ margin: 0, fontSize: 15, fontWeight: 700 }}>Пресет маршрутизации трафика</h3>
        </div>
        <p className="muted small" style={{ margin: '4px 0 0', fontSize: 12 }}>
          Выберите поведение игрового режима для выбранных устройств (механизм аналогичен XKeen UI)
        </p>
      </div>
      <span className="badge" style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', fontSize: 11 }}>
        {cfg.mode === 'bypass_ru' ? 'Обход RU' : cfg.mode === 'compatibility' ? '100% Туннель' : 'GeoSite игры'}
      </span>
    </div>

    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))', gap: 12 }}>
      {/* Пресет 1: Все в прокси кроме RU */}
      <div
        onClick={() => handleModeChange('bypass_ru')}
        style={{
          padding: '14px 16px',
          borderRadius: 8,
          cursor: 'pointer',
          background: cfg.mode === 'bypass_ru' ? 'rgba(56, 189, 248, 0.12)' : 'rgba(255, 255, 255, 0.02)',
          border: cfg.mode === 'bypass_ru' ? '1px solid rgba(56, 189, 248, 0.5)' : '1px solid rgba(255, 255, 255, 0.08)',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          transition: 'all 0.15s ease',
        }}
      >
        <div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input type="radio" checked={cfg.mode === 'bypass_ru'} onChange={() => {}} />
              <b style={{ fontSize: 13.5, color: '#e2e8f0' }}>Все в прокси кроме RU</b>
            </div>
            <span className="badge" style={{ background: 'rgba(34, 197, 94, 0.15)', color: '#4ade80', fontSize: 10 }}>
              ⭐ Рекомендуется
            </span>
          </div>
          <p className="muted small" style={{ margin: 0, lineHeight: 1.45, fontSize: 11.5 }}>
            Весь интернет-трафик и игры идут через туннель, а российские домены (.RU, госуслуги, банки) и GeoIP RU идут напрямую (DIRECT).
          </p>
        </div>
        <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
          <span style={{ fontSize: 11, color: cfg.mode === 'bypass_ru' ? '#38bdf8' : '#94a3b8', fontWeight: 600 }}>
            {cfg.mode === 'bypass_ru' ? '✓ Выбранный пресет' : 'Выбрать пресет'}
          </span>
          <button
            type="button"
            className={`btn sm ${cfg.enabled && cfg.mode === 'bypass_ru' ? 'btn-danger' : 'btn-primary'}`}
            onClick={(e) => {
              e.stopPropagation()
              if (cfg.enabled && cfg.mode === 'bypass_ru') {
                handleToggle(false)
              } else {
                handleToggle(true, 'bypass_ru')
              }
            }}
            disabled={toggling}
            style={{ fontSize: 11 }}
          >
            {cfg.enabled && cfg.mode === 'bypass_ru' ? '⏹ Отключить режим' : '▶ Включить обход RU'}
          </button>
        </div>
      </div>

      {/* Пресет 2: Все напрямую кроме игр */}
      <div
        onClick={() => handleModeChange('known_services')}
        style={{
          padding: '14px 16px',
          borderRadius: 8,
          cursor: 'pointer',
          background: cfg.mode === 'known_services' ? 'rgba(168, 85, 247, 0.12)' : 'rgba(255, 255, 255, 0.02)',
          border: cfg.mode === 'known_services' ? '1px solid rgba(168, 85, 247, 0.5)' : '1px solid rgba(255, 255, 255, 0.08)',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          transition: 'all 0.15s ease',
        }}
      >
        <div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input type="radio" checked={cfg.mode === 'known_services'} onChange={() => {}} />
              <b style={{ fontSize: 13.5, color: '#e2e8f0' }}>Все напрямую кроме игр</b>
            </div>
            <span className="badge" style={{ background: 'rgba(168, 85, 247, 0.15)', color: '#c084fc', fontSize: 10 }}>
              🕹️ GeoSite
            </span>
          </div>
          <p className="muted small" style={{ margin: 0, lineHeight: 1.45, fontSize: 11.5 }}>
            Обычный трафик идет напрямую. В туннель направляются только известные игровые платформы (Steam, Discord, Xbox, PSN) и <code>category-games</code>.
          </p>
          <div
            style={{
              marginTop: 6,
              padding: '4px 6px',
              borderRadius: 4,
              background: 'rgba(234, 179, 8, 0.1)',
              color: '#fde047',
              fontSize: 10.5,
              lineHeight: 1.35,
            }}
          >
            ⚠️ <b>Важно:</b> этот режим <b>не охватывает неизвестные игры</b>. Роутер не может классифицировать закрытые игровые протоколы без доменов.
          </div>
        </div>
        <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
          <span style={{ fontSize: 11, color: cfg.mode === 'known_services' ? '#c084fc' : '#94a3b8', fontWeight: 600 }}>
            {cfg.mode === 'known_services' ? '✓ Выбранный пресет' : 'Выбрать пресет'}
          </span>
          <button
            type="button"
            className={`btn sm ${cfg.enabled && cfg.mode === 'known_services' ? 'btn-danger' : 'btn-secondary'}`}
            onClick={(e) => {
              e.stopPropagation()
              if (cfg.enabled && cfg.mode === 'known_services') {
                handleToggle(false)
              } else {
                handleToggle(true, 'known_services')
              }
            }}
            disabled={toggling}
            style={{ fontSize: 11 }}
          >
            {cfg.enabled && cfg.mode === 'known_services' ? '⏹ Отключить известные сервисы' : '▶ Включить режим известных сервисов'}
          </button>
        </div>
      </div>

      {/* Пресет 3: 100% Всё в прокси */}
      <div
        onClick={() => handleModeChange('compatibility')}
        style={{
          padding: '14px 16px',
          borderRadius: 8,
          cursor: 'pointer',
          background: cfg.mode === 'compatibility' ? 'rgba(245, 158, 11, 0.12)' : 'rgba(255, 255, 255, 0.02)',
          border: cfg.mode === 'compatibility' ? '1px solid rgba(245, 158, 11, 0.5)' : '1px solid rgba(255, 255, 255, 0.08)',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          transition: 'all 0.15s ease',
        }}
      >
        <div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <input type="radio" checked={cfg.mode === 'compatibility'} onChange={() => {}} />
              <b style={{ fontSize: 13.5, color: '#e2e8f0' }}>100% Всё в прокси</b>
            </div>
            <span className="badge" style={{ background: 'rgba(245, 158, 11, 0.15)', color: '#fbbf24', fontSize: 10 }}>
              🔒 Full Proxy
            </span>
          </div>
          <p className="muted small" style={{ margin: 0, lineHeight: 1.45, fontSize: 11.5 }}>
            100% трафика устройства без исключений идет через игровой узел. Решает проблемы со Strict NAT (Type 3) и блокировками серверов консолей.
          </p>
        </div>
        <div style={{ marginTop: 10, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 6 }}>
          <span style={{ fontSize: 11, color: cfg.mode === 'compatibility' ? '#fbbf24' : '#94a3b8', fontWeight: 600 }}>
            {cfg.mode === 'compatibility' ? '✓ Выбранный пресет' : 'Выбрать пресет'}
          </span>
          <button
            type="button"
            className={`btn sm ${cfg.enabled && cfg.mode === 'compatibility' ? 'btn-danger' : 'btn-primary'}`}
            onClick={(e) => {
              e.stopPropagation()
              if (cfg.enabled && cfg.mode === 'compatibility') {
                handleToggle(false)
              } else {
                handleToggle(true, 'compatibility')
              }
            }}
            disabled={toggling}
            style={{ fontSize: 11 }}
          >
            {cfg.enabled && cfg.mode === 'compatibility' ? '⏹ Отключить совместимость' : '▶ Включить полную совместимость'}
          </button>
        </div>
      </div>
    </div>
  </div>
  )
}
