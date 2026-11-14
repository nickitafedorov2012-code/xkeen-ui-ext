import { useState } from 'react'
import { detectWhitelistServers, applyDetectedWhitelistServers } from '../../api'
import type { AppSettings, ServerInfo, ProviderInfo } from '../../types'
import NumberInput from '../NumberInput'

interface WhitelistDetectorSectionProps {
  settings: AppSettings
  servers: ServerInfo[]
  providers: ProviderInfo[]
  patch: (fn: (s: AppSettings) => void) => void
  notify: (msg: string, isError?: boolean) => void
  refresh?: () => void
}

export function WhitelistDetectorSection({
  settings,
  servers,
  providers,
  patch,
  notify,
  refresh,
}: WhitelistDetectorSectionProps) {
  const [settingsDetectorModalOpen, setSettingsDetectorModalOpen] = useState(false)
  const [settingsDetectedClassifier, setSettingsDetectedClassifier] = useState<{
    detectedServerIds: string[]
    detectedProviderIds: string[]
    details: Array<{ id: string; name: string; reason: string; type: 'server' | 'provider' }>
  } | null>(null)
  const [settingsClassifierApplying, setSettingsClassifierApplying] = useState(false)

              const det = settings.failover.whitelist_detector || {
                enabled: false,
                monitored_provider: '',
                exclude_ru_nodes: true,
                target_bs_provider: '',
                threshold_mode: 'percent',
                trigger_offline_percent: 80,
                trigger_offline_count: 75,
                max_acceptable_ping_ms: 450,
                exit_offline_percent: 45,
                exit_offline_count: 48,
                check_interval_mins: 10,
                auto_restore_vps: true,
              }

              const ensureDetector = (s: AppSettings) => {
                if (!s.failover.whitelist_detector) {
                  s.failover.whitelist_detector = {
                    enabled: false,
                    monitored_provider: '',
                    exclude_ru_nodes: true,
                    target_bs_provider: '',
                    threshold_mode: 'percent',
                    trigger_offline_percent: 80,
                    trigger_offline_count: 75,
                    max_acceptable_ping_ms: 450,
                    exit_offline_percent: 45,
                    exit_offline_count: 48,
                    check_interval_mins: 10,
                    auto_restore_vps: true,
                  }
                }
                return s.failover.whitelist_detector
              }

              const handleOpenSettingsClassifier = () => {
                const detected = detectWhitelistServers(servers, providers)
                // Если узлы пришли без флага от сервера, дополняем локальными тегами (fallback)
                if (detected.detectedServerIds.length === 0 && detected.detectedProviderIds.length === 0) {
                  const checkTagFallback = (s: string) => {
                    const l = s.toLowerCase()
                    return (
                      l.includes('[бс]') || l.includes('(бс)') || l.includes('[bs]') || l.includes('(bs)') ||
                      l.includes('[wl]') || l.includes('(wl)') || /\bwhitelist\b/i.test(l)
                    )
                  }
                  for (const p of providers) {
                    if (checkTagFallback(p.name) || checkTagFallback(p.id)) {
                      if (!detected.detectedProviderIds.includes(p.id)) {
                        detected.detectedProviderIds.push(p.id)
                        detected.details.push({ id: p.id, name: p.name, reason: 'Подписка [WL]', type: 'provider' })
                      }
                    }
                  }
                  for (const s of servers) {
                    if (checkTagFallback(s.name) || checkTagFallback(s.id)) {
                      if (!detected.detectedServerIds.includes(s.id)) {
                        detected.detectedServerIds.push(s.id)
                        detected.details.push({ id: s.id, name: s.name, reason: 'Узел [БС]', type: 'server' })
                      }
                    }
                  }
                }
                setSettingsDetectedClassifier(detected)
                setSettingsDetectorModalOpen(true)
              }

              const handleApplySettingsClassifier = async () => {
                if (!settingsDetectedClassifier || settingsClassifierApplying) return
                setSettingsClassifierApplying(true)
                try {
                  const res = await applyDetectedWhitelistServers(
                    settingsDetectedClassifier.detectedServerIds,
                    settingsDetectedClassifier.detectedProviderIds
                  )
                  notify(
                    `Применено: серверов БС — ${res.whitelist_servers.length}, подписок БС — ${res.whitelist_providers.length}`
                  )
                  setSettingsDetectorModalOpen(false)
                  refresh?.()
                } catch (e: any) {
                  notify(e?.message || 'Ошибка применения белых списков', true)
                } finally {
                  setSettingsClassifierApplying(false)
                }
              }

              return (
                <div
                  style={{
                    marginTop: 6,
                    padding: 14,
                    background: 'rgba(168, 85, 247, 0.04)',
                    border: '1px solid rgba(168, 85, 247, 0.25)',
                    borderRadius: 10,
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 12,
                  }}
                  data-testid="settings-whitelist-detector-card"
                >
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 8 }}>
                    <div>
                      <b style={{ display: 'block', fontSize: 13.5, color: '#c084fc' }}>
                        🛡️ Автодетекция блокировок ТСПУ и режим [БС]
                      </b>
                      <span className="muted small">
                        Постоянный контроль доступности зарубежных серверов и мгновенное переключение на [БС] при изоляции
                      </span>
                    </div>
                    <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
                      <label className="switch" title="Включить детектор белых списков">
                        <input
                          type="checkbox"
                          checked={Boolean(det.enabled)}
                          onChange={(e) =>
                            patch((s) => {
                              const d = ensureDetector(s)
                              d.enabled = e.target.checked
                            })
                          }
                          data-testid="settings-whitelist-detector-toggle"
                        />
                        <span className="slider" />
                      </label>
                      <span style={{ fontSize: 13, minWidth: 64, color: det.enabled ? '#c084fc' : 'var(--muted)', fontWeight: 600 }}>
                        {det.enabled ? 'Включён' : 'Выключен'}
                      </span>
                    </div>
                  </div>

                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))', gap: 10 }}>
                    {/* Контролируемый провайдер */}
                    <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4, width: '100%' }}>
                      <span style={{ fontSize: 12, fontWeight: 500 }}>Контролируемый пул (зарубежный VPS)</span>
                      <select
                        className="input"
                        value={det.monitored_provider || ''}
                        onChange={(e) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.monitored_provider = e.target.value
                          })
                        }
                        style={{ width: '100%', boxSizing: 'border-box' }}
                        data-testid="settings-detector-monitored-provider"
                      >
                        <option value="">Авто (все зарубежные узлы Tier 1)</option>
                        {providers.map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.name || p.id} ({p.count} узлов)
                          </option>
                        ))}
                      </select>
                    </label>

                    {/* Целевой сервер / провайдер БС */}
                    <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4, width: '100%' }}>
                      <span style={{ fontSize: 12, fontWeight: 500 }}>Целевой шлюз белых списков [БС]</span>
                      <select
                        className="input"
                        value={det.target_bs_provider || ''}
                        onChange={(e) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.target_bs_provider = e.target.value
                          })
                        }
                        style={{ width: '100%', boxSizing: 'border-box' }}
                        data-testid="settings-detector-target-provider"
                      >
                        <option value="">Авто (серверы, помеченные [БС])</option>
                        {providers.map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.name || p.id} {p.is_whitelist ? '⭐ [БС]' : ''}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>

                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: 10 }}>
                    {/* Порог срабатывания % */}
                    <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                      <span style={{ fontSize: 12 }}>Порог изоляции (%)</span>
                      <NumberInput
                        min={10}
                        max={100}
                        step={5}
                        fallback={80}
                        value={det.trigger_offline_percent ?? 80}
                        onChange={(val) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.trigger_offline_percent = val
                          })
                        }
                      />
                    </label>

                    {/* Порог выхода % */}
                    <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                      <span style={{ fontSize: 12 }}>Порог снятия ТСПУ (%)</span>
                      <NumberInput
                        min={5}
                        max={90}
                        step={5}
                        fallback={45}
                        value={det.exit_offline_percent ?? 45}
                        onChange={(val) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.exit_offline_percent = val
                          })
                        }
                      />
                    </label>

                    {/* Макс допустимый пинг */}
                    <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                      <span style={{ fontSize: 12 }}>Макс. допустимый пинг (мс)</span>
                      <NumberInput
                        min={100}
                        max={2000}
                        step={50}
                        fallback={450}
                        value={det.max_acceptable_ping_ms ?? 450}
                        onChange={(val) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.max_acceptable_ping_ms = val
                          })
                        }
                      />
                    </label>

                    {/* Интервал проверки в минутах */}
                    <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                      <span style={{ fontSize: 12 }}>Интервал сканирования (мин)</span>
                      <NumberInput
                        min={1}
                        max={1440}
                        step={1}
                        fallback={10}
                        value={det.check_interval_mins ?? 10}
                        onChange={(val) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.check_interval_mins = val
                          })
                        }
                      />
                    </label>

                    {/* Пул проверки зарубежных узлов (sample_size) */}
                    <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4 }}>
                      <span style={{ fontSize: 12 }}>Пул проверки зарубежных узлов</span>
                      <select
                        className="input"
                        value={
                          det.sample_size === undefined || det.sample_size === 0
                            ? '0'
                            : det.sample_size === 24
                            ? '24'
                            : det.sample_size === 50
                            ? '50'
                            : 'custom'
                        }
                        onChange={(e) => {
                          const val = e.target.value
                          patch((s) => {
                            const d = ensureDetector(s)
                            if (val === '0') d.sample_size = 0
                            else if (val === '24') d.sample_size = 24
                            else if (val === '50') d.sample_size = 50
                            else if (val === 'custom') {
                              if (!d.sample_size || d.sample_size === 0 || d.sample_size === 24 || d.sample_size === 50) {
                                d.sample_size = 30
                              }
                            }
                          })
                        }}
                        data-testid="settings-detector-sample-size-select"
                      >
                        <option value="0">Все узлы подписки (100% охват, 93+ узлов)</option>
                        <option value="24">Быстрый срез: 24 узла (2-3 сек)</option>
                        <option value="50">Расширенный срез: 50 узлов</option>
                        <option value="custom">Свой размер пула…</option>
                      </select>
                    </label>
                  </div>

                  {/* Если выбран произвольный размер пула узлов */}
                  {Boolean(
                    det.sample_size !== undefined &&
                    det.sample_size !== 0 &&
                    det.sample_size !== 24 &&
                    det.sample_size !== 50
                  ) && (
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                      <label className="row" style={{ margin: 0, flexDirection: 'column', alignItems: 'flex-start', gap: 4, maxWidth: 220 }}>
                        <span style={{ fontSize: 12 }}>Произвольное число узлов (шт.)</span>
                        <NumberInput
                          min={1}
                          max={500}
                          step={5}
                          fallback={30}
                          value={det.sample_size ?? 30}
                          onChange={(val) =>
                            patch((s) => {
                              const d = ensureDetector(s)
                              d.sample_size = val
                            })
                          }
                          data-testid="settings-detector-sample-size-custom-input"
                        />
                      </label>
                      <span className="muted small" style={{ fontSize: 11, marginTop: 18 }}>
                        Проверяет первые N активных зарубежных серверов провайдера для ускорения детекции ТСПУ
                      </span>
                    </div>
                  )}

                  {/* Подсказка/бейдж для размера пула */}
                  <div
                    style={{
                      fontSize: 11.5,
                      padding: '6px 10px',
                      borderRadius: 6,
                      background: 'rgba(168, 85, 247, 0.08)',
                      border: '1px solid rgba(168, 85, 247, 0.2)',
                      color: 'var(--text-bright)',
                      display: 'flex',
                      alignItems: 'center',
                      gap: 6,
                    }}
                    data-testid="settings-detector-sample-size-hint"
                  >
                    <span>💡</span>
                    <span>
                      {(det.sample_size === undefined || det.sample_size === 0)
                        ? 'Режим 100% охвата: опрашивает все узлы провайдера. Точнее всего фиксирует ковровые блокировки ТСПУ.'
                        : `Режим среза (${det.sample_size} узлов): проверяет репрезентативную выборку серверов, снижая нагрузку на роутер и давая вердикт за секунды.`}
                    </span>
                  </div>

                  {/* Чекбоксы: Отсев RU нод и Автовозврат на VPS */}
                  <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap', alignItems: 'center' }}>
                    <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, cursor: 'pointer', fontSize: 12.5 }}>
                      <input
                        type="checkbox"
                        checked={det.exclude_ru_nodes ?? true}
                        onChange={(e) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.exclude_ru_nodes = e.target.checked
                          })
                        }
                        data-testid="settings-detector-exclude-ru"
                      />
                      <span>Исключать ноды РФ из контрольной группы зарубежных узлов</span>
                    </label>

                    <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8, cursor: 'pointer', fontSize: 12.5 }}>
                      <input
                        type="checkbox"
                        checked={det.auto_restore_vps ?? true}
                        onChange={(e) =>
                          patch((s) => {
                            const d = ensureDetector(s)
                            d.auto_restore_vps = e.target.checked
                          })
                        }
                        data-testid="settings-detector-auto-restore"
                      />
                      <span>Авто-возврат на обычный VPS при снятии блокировки ТСПУ</span>
                    </label>
                  </div>

                  {/* Кнопка автоопределения БС узлов */}
                  <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginTop: 6, flexWrap: 'wrap', gap: 8, padding: '8px 10px', background: 'rgba(255, 255, 255, 0.02)', borderRadius: 8, border: '1px solid var(--border)' }}>
                    <button
                      type="button"
                      className="btn sm"
                      onClick={handleOpenSettingsClassifier}
                      data-testid="settings-detector-classify-btn"
                      title="Автоматически распознать серверы с метками БС и добавить в список"
                    >
                      ✨ Автоопределение серверов БС
                    </button>
                    <span className="muted small" style={{ fontSize: 11, flex: 1, minWidth: 200 }}>
                      Находит конкретные узлы обхода (теги [БС], Whitelist) и помечает их как Tier 2
                    </span>
                  </div>

                  {/* Модальное окно классификатора внутри настроек */}
                  {settingsDetectorModalOpen && settingsDetectedClassifier && (
                    <div
                      className="modal-backdrop"
                      style={{
                        position: 'fixed',
                        top: 0,
                        left: 0,
                        right: 0,
                        bottom: 0,
                        background: 'rgba(0, 0, 0, 0.7)',
                        backdropFilter: 'blur(3px)',
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        zIndex: 9999,
                        padding: 16,
                      }}
                      onClick={() => setSettingsDetectorModalOpen(false)}
                    >
                      <div
                        className="modal-content card"
                        style={{
                          maxWidth: 580,
                          width: '100%',
                          background: 'var(--card-bg, #1e293b)',
                          border: '1px solid var(--border, rgba(255, 255, 255, 0.1))',
                          borderRadius: 12,
                          padding: 20,
                        }}
                        onClick={(e) => e.stopPropagation()}
                        data-testid="settings-detector-classifier-modal"
                      >
                        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
                          <h3 style={{ margin: 0, fontSize: 16, display: 'flex', alignItems: 'center', gap: 8 }}>
                            <span>✨</span> Результаты классификации БС
                          </h3>
                          <button
                            type="button"
                            className="btn ghost sm"
                            onClick={() => setSettingsDetectorModalOpen(false)}
                          >
                            ✕
                          </button>
                        </div>

                        {settingsDetectedClassifier.details.length === 0 ? (
                          <div style={{ padding: 16, textAlign: 'center', color: 'var(--muted)', fontSize: 13 }}>
                            ℹ️ Теги белых списков не обнаружены среди доступных узлов и подписок.
                          </div>
                        ) : (
                          <div style={{ maxHeight: 280, overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: 6, marginBottom: 16 }}>
                            {settingsDetectedClassifier.details.map((d) => (
                              <div
                                key={`${d.type}-${d.id}`}
                                style={{
                                  display: 'flex',
                                  alignItems: 'center',
                                  justifyContent: 'space-between',
                                  gap: 10,
                                  padding: '8px 12px',
                                  background: 'rgba(255, 255, 255, 0.03)',
                                  border: '1px solid rgba(255, 255, 255, 0.08)',
                                  borderRadius: 6,
                                  fontSize: 12,
                                }}
                              >
                                <div style={{ display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden' }}>
                                  <span className="badge" style={{ background: d.type === 'provider' ? 'rgba(56, 189, 248, 0.2)' : 'rgba(168, 85, 247, 0.2)', color: d.type === 'provider' ? '#38bdf8' : '#c084fc', fontSize: 10 }}>
                                    {d.type === 'provider' ? 'Подписка' : 'Сервер'}
                                  </span>
                                  <b style={{ color: 'var(--text-bright)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                    {d.name || d.id}
                                  </b>
                                </div>
                                <span className="muted small" style={{ fontSize: 10.5, whiteSpace: 'nowrap' }}>
                                  {d.reason}
                                </span>
                              </div>
                            ))}
                          </div>
                        )}

                        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 16 }}>
                          <button
                            type="button"
                            className="btn ghost"
                            onClick={() => setSettingsDetectorModalOpen(false)}
                          >
                            Отмена
                          </button>
                          <button
                            type="button"
                            className="btn primary"
                            disabled={settingsDetectedClassifier.details.length === 0 || settingsClassifierApplying}
                            onClick={handleApplySettingsClassifier}
                            data-testid="settings-detector-apply-classifier-btn"
                          >
                            {settingsClassifierApplying ? 'Применение…' : `Применить в 1 клик (${settingsDetectedClassifier.details.length})`}
                          </button>
                        </div>
                      </div>
                    </div>
                  )}
                </div>
              )
}