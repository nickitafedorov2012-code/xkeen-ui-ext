import { useState } from 'react'
import { PRESET_CATEGORIES, type PresetCategory, type PresetService } from '../constants/presets'
import { apiGet, apiPost } from '../api'

interface PresetCatalogModalProps {
  isOpen?: boolean
  onClose: () => void
  onSuccess?: () => void
  onApplyPreset?: (domains: string[], target: 'force' | 'direct') => void
  notify: (msg: string, error?: boolean) => void
}

export default function PresetCatalogModal({
  isOpen = true,
  onClose,
  onSuccess,
  onApplyPreset,
  notify,
}: PresetCatalogModalProps) {
  const [activeCat, setActiveCat] = useState<string>('ai')
  const [selectedServices, setSelectedServices] = useState<Set<string>>(new Set())
  const [targetType, setTargetType] = useState<'force' | 'direct'>('force')
  const [saving, setSaving] = useState(false)

  const currentCategory: PresetCategory =
    PRESET_CATEGORIES.find((c) => c.id === activeCat) || PRESET_CATEGORIES[0]

  const toggleService = (srvId: string) => {
    setSelectedServices((prev) => {
      const next = new Set(prev)
      if (next.has(srvId)) {
        next.delete(srvId)
      } else {
        next.add(srvId)
      }
      return next
    })
  }

  const selectAllInCategory = () => {
    setSelectedServices((prev) => {
      const next = new Set(prev)
      for (const srv of currentCategory.services) {
        next.add(srv.id)
      }
      return next
    })
  }

  const deselectAllInCategory = () => {
    setSelectedServices((prev) => {
      const next = new Set(prev)
      for (const srv of currentCategory.services) {
        next.delete(srv.id)
      }
      return next
    })
  }

  const allSelectedDomains = Array.from(selectedServices).flatMap((id) => {
    for (const cat of PRESET_CATEGORIES) {
      const srv = cat.services.find((s) => s.id === id)
      if (srv) return srv.domains
    }
    return []
  })

  const uniqueDomains = Array.from(new Set(allSelectedDomains))

  const handleApply = async () => {
    if (uniqueDomains.length === 0) {
      notify('Выберите хотя бы один сервис для добавления', true)
      return
    }

    if (onApplyPreset) {
      onApplyPreset(uniqueDomains, targetType)
      onClose()
      return
    }

    setSaving(true)
    try {
      // 1. Получаем текущие доменные списки
      const cur = await apiGet<{ direct: string[]; force: string[] }>('domains')
      const targetList = targetType === 'force' ? (cur.force || []) : (cur.direct || [])

      // 2. Объединяем без дубликатов
      const merged = Array.from(new Set([...targetList, ...uniqueDomains]))

      // 3. Сохраняем обновленный список
      await apiPost('domains', {
        direct: targetType === 'direct' ? merged : (cur.direct || []),
        force: targetType === 'force' ? merged : (cur.force || []),
      })

      notify(
        `✓ Успешно добавлено ${uniqueDomains.length} доменов в ${
          targetType === 'force' ? '«Принудительно через прокси»' : '«Напрямую»'
        }`
      )
      onSuccess?.()
      onClose()
    } catch (err: any) {
      notify('Ошибка сохранения правил: ' + err.message, true)
    } finally {
      setSaving(false)
    }
  }

  if (!isOpen) return null

  return (
    <div className="modal-backdrop">
      <div className="modal-card presets-card">
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-icon">📦</span>
            <h2>Каталог шаблонов и пресетов правил</h2>
          </div>
          <button className="btn btn-sm" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="presets-card-body">
          <p className="muted" style={{ margin: '0 0 14px' }}>
            Выберите популярные сервисы для добавления их доменов и CDN в правила маршрутизации в 1 клик.
          </p>

          {/* Навигация по категориям */}
          <div className="presets-category-tabs">
          {PRESET_CATEGORIES.map((cat) => (
            <button
              key={cat.id}
              className={`preset-tab ${activeCat === cat.id ? 'active' : ''}`}
              onClick={() => setActiveCat(cat.id)}
            >
              <span className="preset-tab-icon">{cat.icon}</span>
              <span>{cat.name}</span>
            </button>
          ))}
        </div>

        <div className="presets-controls-bar">
          <div className="presets-select-buttons">
            <button className="btn btn-sm" onClick={selectAllInCategory}>
              ✓ Выбрать все в категории
            </button>
            <button className="btn btn-sm" onClick={deselectAllInCategory}>
              ✕ Снять в категории
            </button>
          </div>

          <div className="presets-target-toggle">
            <span className="muted">Добавить в:</span>
            <label className="radio-label">
              <input
                type="radio"
                name="targetType"
                checked={targetType === 'force'}
                onChange={() => setTargetType('force')}
              />
              <span>🔒 Через прокси (PROXY)</span>
            </label>
            <label className="radio-label">
              <input
                type="radio"
                name="targetType"
                checked={targetType === 'direct'}
                onChange={() => setTargetType('direct')}
              />
              <span>⏭ Напрямую (DIRECT)</span>
            </label>
          </div>
        </div>

        {/* Список сервисов */}
        <div className="presets-services-grid">
          {currentCategory.services.map((srv: PresetService) => {
            const isSelected = selectedServices.has(srv.id)
            return (
              <div
                key={srv.id}
                className={`preset-service-card ${isSelected ? 'selected' : ''}`}
                onClick={() => toggleService(srv.id)}
              >
                <div className="preset-service-header">
                  <span className="preset-service-icon">{srv.icon}</span>
                  <div className="preset-service-info">
                    <h4>{srv.name}</h4>
                    <p className="muted">{srv.description}</p>
                  </div>
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => {}} // Обрабатывается кликом по карточке
                    className="preset-service-checkbox"
                  />
                </div>

                <div className="preset-service-domains">
                  {srv.domains.map((d) => (
                    <span key={d} className="domain-pill">
                      {d}
                    </span>
                  ))}
                </div>
              </div>
            )
          })}
        </div>
        </div>

        {/* Футер модалки */}
        <div className="modal-footer">
          <div className="presets-summary muted">
            Выбрано сервисов: <b>{selectedServices.size}</b> | Уникальных доменов: <b>{uniqueDomains.length}</b>
          </div>

          <div style={{ display: 'flex', gap: '8px' }}>
            <button className="btn btn-secondary" onClick={onClose}>
              Отмена
            </button>
            <button
              className="btn btn-primary"
              disabled={uniqueDomains.length === 0 || saving}
              onClick={handleApply}
            >
              {saving ? '⏳ Применение…' : `✨ Добавить ${uniqueDomains.length} доменов`}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
