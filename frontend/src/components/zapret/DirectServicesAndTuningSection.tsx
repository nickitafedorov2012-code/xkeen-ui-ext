import type { Dispatch, ReactNode, SetStateAction } from 'react'
import type { ZapretFeatures, ZapretStatus } from '../../types'
import { renderMicroSpinner } from './zapretHelpers'

type RenderStrategyCardFn = (
  key: keyof ZapretFeatures,
  title: string,
  badgeText: string,
  desc: string,
  activeInfo: string,
  inactiveInfo: string,
  tags?: string[],
  supportedEngines?: ('v1' | 'v2')[]
) => ReactNode

export function DirectServicesSection({
  renderStrategyCard,
}: {
  renderStrategyCard: RenderStrategyCardFn
}) {
  return (
      <section className="card" style={{ padding: '22px 24px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, marginBottom: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                🛡️ Службы и сервисы (прямой обход DPI без VPS)
              </h3>
              <span
                className="badge"
                style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', border: '1px solid rgba(56, 189, 248, 0.3)' }}
              >
                Независимые блоки
              </span>
            </div>
            <div className="muted small" style={{ marginTop: 3 }}>
              Каждый сервис можно выключить или включить отдельно. Трафик включенных сервисов десинхронизируется локально и не расходует лимиты VPS.
            </div>
          </div>
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(340px, 1fr))', gap: 14 }}>
          {/* 1. YouTube Direct */}
          {renderStrategyCard(
            'hybrid_youtube',
            '🎥 YouTube Direct (без VPS)',
            'YOUTUBE DIRECT',
            'Направляет видеопотоки YouTube и кэш-серверы googlevideo напрямую через локальный nfqws в обход прокси. Экономит зарубежный трафик и ускоряет 4K/8K.',
            'DIRECT через Zapret (без VPS)',
            'через VLESS-прокси',
            ['googlevideo.com', 'youtube.com', 'ytimg.com', 'ggpht.com']
          )}

          {/* 2. Discord Web & Chat */}
          {renderStrategyCard(
            'hybrid_discord',
            '💬 Discord Web & Chat',
            'DISCORD DIRECT',
            'Прямое подключение к текстовым серверам, каналам и медиафайлам Discord напрямую через Zapret. Минимальный домашний пинг и быстрая загрузка картинок.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['discord.com', 'discord.gg', 'discordapp.com', 'discord.media']
          )}

          {/* 3. GitHub (Релизы, исходники & аватары) */}
          {renderStrategyCard(
            'bypass_github',
            '🐙 GitHub (Релизы & Исходники)',
            'GITHUB DIRECT',
            'Прямой обход блокировок и замедлений GitHub: моментальный git clone, высокая скорость загрузки релизов, raw-файлов и аватаров без нагрузки на VPS.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['github.com', 'raw.githubusercontent.com', 'assets-cdn.github.com', 'objects.githubusercontent.com']
          )}

          {/* 4. Торренты & Трекеры */}
          {renderStrategyCard(
            'bypass_torrents',
            '🧲 Торренты & Трекеры',
            'TRACKERS DIRECT',
            'Прямой доступ к анонсам трекеров и скачиванию .torrent файлов: RuTracker, Kinozal, Rutor, Flibusta, NNMClub, Torlook. Работает на максимальной скорости провайдера.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['rutracker.org', 'kinozal.tv', 'rutor.info', 'flibusta.is', 'nnmclub.to']
          )}

          {/* 5. 🔞 18+ Контент */}
          {renderStrategyCard(
            'bypass_adult',
            '🔞 18+ Контент',
            'ADULT DIRECT',
            'Прямой локальный обход блокировок ТСПУ для популярных сайтов 18+ (Pornhub, Xvideos, XHamster, XNXX, RedTube, YouPorn) и их медиа-CDN без расхода трафика VPS.',
            'DIRECT через Zapret',
            'через стандартный маршрут',
            ['pornhub.com', 'xvideos.com', 'xhamster.com', 'xnxx.com', 'redtube.com', 'youporn.com']
          )}

          {/* 6. Универсальный веб-обход (Hostlist) */}
          {renderStrategyCard(
            'general_bypass',
            '🌐 Универсальный веб-обход (Hostlist)',
            'HOSTLIST',
            'Обход блокировок по системному списку доменов (/opt/etc/zapret/zapret-hosts.txt). Обычные сайты и банки не затрагиваются.',
            'zapret-hosts.txt (cutoff=d4)',
            'без фильтрации общего веб',
            ['/opt/etc/zapret/zapret-hosts.txt']
          )}
        </div>
      </section>
  )
}

interface DpiTuningSectionProps {
  status: ZapretStatus | null
  busy: boolean
  pendingKeys: Set<string>
  isInstalled: boolean
  activeEngine: 'v1' | 'v2'
  setShowConfigEditor: Dispatch<SetStateAction<boolean>>
  handleApplyPreset: (presetId: string) => void
  handleResetFeatures: () => void
  renderStrategyCard: RenderStrategyCardFn
}

export function DpiTuningSection({
  status,
  busy,
  pendingKeys,
  isInstalled,
  activeEngine,
  setShowConfigEditor,
  handleApplyPreset,
  handleResetFeatures,
  renderStrategyCard,
}: DpiTuningSectionProps) {
  return (
      <section className="card" style={{ padding: '22px 24px' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12, marginBottom: 16 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <h3 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>
                ⚡ Режимы десинхронизации (DPI Tuning) & Быстрые наборы
              </h3>
              <span
                className="badge"
                style={{ background: 'rgba(56, 189, 248, 0.15)', color: '#38bdf8', border: '1px solid rgba(56, 189, 248, 0.3)' }}
              >
                Тонкая настройка
              </span>
            </div>
            <div className="muted small" style={{ marginTop: 3 }}>
              Специальные параметры nfqws для пробития глубоких блокировок ТСПУ и изоляции защищенных ресурсов
            </div>
          </div>

          {/* БЫСТРЫЕ НАБОРЫ */}
          {(() => {
            const activePreset = status?.preset || 'custom'
            return (
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                <button
                  type="button"
                  className={`btn sm ${activePreset === 'youtube' ? 'primary' : 'ghost'}`}
                  disabled={busy || !isInstalled}
                  onClick={() => handleApplyPreset('youtube')}
                  title="Только YouTube Turbo (Google GGC Direct)"
                  style={activePreset === 'youtube' ? { border: '1px solid #38bdf8', color: '#38bdf8', background: 'rgba(56, 189, 248, 0.15)' } : {}}
                >
                  {pendingKeys.has('preset:youtube') && <span style={{ marginRight: 6 }}>{renderMicroSpinner('#38bdf8', 11)}</span>}
                  📺 Только YouTube
                </button>

                <button
                  type="button"
                  className={`btn sm ${activePreset === 'discord' ? 'primary' : 'ghost'}`}
                  disabled={busy || !isInstalled}
                  onClick={() => handleApplyPreset('discord')}
                  title="Только Discord Web и Discord Voice UDP"
                  style={activePreset === 'discord' ? { border: '1px solid #38bdf8', color: '#38bdf8', background: 'rgba(56, 189, 248, 0.15)' } : {}}
                >
                  {pendingKeys.has('preset:discord') && <span style={{ marginRight: 6 }}>{renderMicroSpinner('#38bdf8', 11)}</span>}
                  💬 Только Discord
                </button>

                <button
                  type="button"
                  className={`btn sm ${activePreset === 'gamer' ? 'primary' : 'ghost'}`}
                  disabled={busy || !isInstalled}
                  onClick={() => handleApplyPreset('gamer')}
                  title="YouTube Turbo + Discord Web + Discord Voice RTC + GitHub + Торренты"
                  style={activePreset === 'gamer' ? { border: '1px solid #38bdf8', color: '#38bdf8', background: 'rgba(56, 189, 248, 0.15)' } : {}}
                >
                  {pendingKeys.has('preset:gamer') && <span style={{ marginRight: 6 }}>{renderMicroSpinner('#38bdf8', 11)}</span>}
                  🎮 Медиа и Игры
                </button>

                <button
                  type="button"
                  className={`btn sm ${activePreset === 'aggressive' ? 'primary' : 'ghost'}`}
                  disabled={busy || !isInstalled || activeEngine === 'v1'}
                  onClick={() => handleApplyPreset('aggressive')}
                  title={
                    activeEngine === 'v1'
                      ? 'Пресет «🔥 Агрессивный» доступен только для Zapret 2.0 (Modern nfqws2)'
                      : 'Все стратегии + Агрессивный режим для жестких ТСПУ'
                  }
                  style={{
                    ...(activePreset === 'aggressive'
                      ? { border: '1px solid #38bdf8', color: '#38bdf8', background: 'rgba(56, 189, 248, 0.15)' }
                      : {}),
                    ...(activeEngine === 'v1'
                      ? { opacity: 0.45, filter: 'grayscale(0.8)', cursor: 'not-allowed' }
                      : {}),
                  }}
                >
                  {pendingKeys.has('preset:aggressive') && <span style={{ marginRight: 6 }}>{renderMicroSpinner('#38bdf8', 11)}</span>}
                  🔥 Агрессивный {activeEngine === 'v1' ? '(v2)' : ''}
                </button>

                <button
                  type="button"
                  className={`btn sm ${activePreset === 'custom' && (status?.preset === 'custom' || status?.config?.includes('NFQWS_ARGS')) ? 'primary' : 'ghost'}`}
                  disabled={busy || !isInstalled}
                  onClick={() => setShowConfigEditor(true)}
                  title="Ручное редактирование zapret.conf и произвольные аргументы nfqws2"
                  style={activePreset === 'custom' && (status?.preset === 'custom' || status?.config?.includes('NFQWS_ARGS')) ? { border: '1px solid #a855f7', color: '#c084fc', background: 'rgba(168, 85, 247, 0.15)' } : {}}
                >
                  ⚙️ Пользовательский (Custom)
                </button>

                <button
                  type="button"
                  className={`btn sm ${activePreset === 'default' || activePreset === 'general' ? 'primary' : 'ghost'}`}
                  disabled={busy || !isInstalled}
                  onClick={() => handleApplyPreset('default')}
                  title="Сбалансированный режим: все сервисы и списки включены"
                  style={activePreset === 'default' || activePreset === 'general' ? { border: '1px solid #22c55e', color: '#22c55e', background: 'rgba(34, 197, 94, 0.15)' } : {}}
                >
                  {pendingKeys.has('preset:default') && <span style={{ marginRight: 6 }}>{renderMicroSpinner('#22c55e', 11)}</span>}
                  ✨ Все сервисы (По умолчанию)
                </button>

                <button
                  type="button"
                  className="btn sm ghost"
                  disabled={busy || !isInstalled}
                  onClick={handleResetFeatures}
                  title="Сбросить все блоки к стандартным значениям"
                >
                  ↺ Сброс
                </button>
              </div>
            )
          })()}
        </div>

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(340px, 1fr))', gap: 14 }}>
          {/* 1. YouTube Turbo */}
          {renderStrategyCard(
            'youtube_turbo',
            '⚡ YouTube Turbo (Fake + Split2)',
            'GGC DIRECT',
            'Десинхронизация ClientHello (fake,split2, pos=1) с repeats=6, fooling=ts и отсечкой cutoff=d4. Пробивает блокировки ТСПУ и устраняет буферизацию с кэшей Google GGC без расхода VPS.',
            'fake,split2 (pos=1, repeats=6, ts, cutoff=d4)',
            'стандартный режим'
          )}

          {/* 2. Discord Voice RTC */}
          {renderStrategyCard(
            'discord_voice_udp',
            '🎙️ Discord Voice RTC (UDP 50000:65535)',
            'RTC VOICE',
            'Перехват голосовых шлюзов Discord в iptables mangle. Устраняет вечный статус «RTC Connecting» и потерю звука в голосовом канале.',
            'UDP 50000:65535 + L7 discord/stun',
            'только TCP/UDP 80/443'
          )}

          {/* 3. Агрессивный режим ТСПУ */}
          {renderStrategyCard(
            'aggressive_dpi',
            '🔥 Агрессивный режим ТСПУ (seqovl + midsld + ts)',
            'ТСПУ BOOST',
            'Перекрытие последовательностей (seqovl=1), сплит по середине SNI (pos=1,midsld), повторы repeats=6 и подделка ts,md5sig. Пробивает жесткие блокировки мобильных и кабельных операторов.',
            'seqovl=1, pos=1,midsld, repeats=6, ts,md5sig',
            'базовые стратегии',
            ['seqovl', 'midsld', 'ts', 'md5sig'],
            ['v2']
          )}

          {/* 4. Изоляция IP-блокировок */}
          {renderStrategyCard(
            'isolated_proxy',
            '🔒 Изоляция IP-блокировок (ChatGPT, Claude, X -> PROXY)',
            'STRICT PROXY',
            'Разделение задач: Zapret обходит только цензуру по SNI (YouTube/Discord/GitHub). Сервисы с жесткой блокировкой по IP (ChatGPT, Claude, Instagram, X/Twitter) гарантированно идут через VPS.',
            'AI и соцсети строго через VPS PROXY',
            'по общим правилам маршрутизации'
          )}
        </div>
      </section>
  )
}