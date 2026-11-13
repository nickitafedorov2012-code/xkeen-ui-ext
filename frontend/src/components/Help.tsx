import { useMemo, useState, type ReactNode } from 'react'
import type { StatusInfo } from '../types'

interface Props {
  status: StatusInfo | null
}

function SectionCard({ title, badge, icon, children }: { title: string; badge?: string; icon: string; children: ReactNode }) {
  return (
    <section className="card" style={{ marginBottom: 16 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12, flexWrap: 'wrap', gap: 8 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: 20 }}>{icon}</span>
          <h2 style={{ margin: 0, fontSize: 16.5, color: 'var(--text)' }}>{title}</h2>
        </div>
        {badge && (
          <span className="badge" style={{ fontSize: 11, background: 'var(--panel-2)', border: '1px solid var(--border)', color: 'var(--accent)' }}>
            {badge}
          </span>
        )}
      </div>
      {children}
    </section>
  )
}

function P({ children }: { children: ReactNode }) {
  return <p style={{ margin: '6px 0', fontSize: 13.5, lineHeight: 1.6, color: 'var(--text)' }}>{children}</p>
}

function K({ children }: { children: ReactNode }) {
  return (
    <code style={{ background: 'var(--panel-2)', border: '1px solid var(--border)', borderRadius: 4, padding: '1px 6px', fontSize: 12.5, color: 'var(--accent)', fontFamily: 'Consolas, monospace' }}>
      {children}
    </code>
  )
}

function FeatureList({ items }: { items: { name: string; desc: ReactNode }[] }) {
  return (
    <ul style={{ listStyle: 'none', padding: 0, margin: '8px 0' }}>
      {items.map((it, idx) => (
        <li
          key={idx}
          style={{
            padding: '10px 12px',
            borderRadius: 8,
            background: idx % 2 === 0 ? 'rgba(255, 255, 255, 0.02)' : 'transparent',
            display: 'flex',
            flexDirection: 'column',
            gap: 4,
            fontSize: 13.5,
            lineHeight: 1.5,
            borderBottom: '1px solid rgba(255, 255, 255, 0.03)',
          }}
        >
          <span style={{ fontWeight: 600, color: 'var(--text)' }}>{it.name}</span>
          <span style={{ color: 'var(--muted)' }}>{it.desc}</span>
        </li>
      ))}
    </ul>
  )
}

function StepList({ steps }: { steps: { step: number; title: string; desc: ReactNode }[] }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10, margin: '12px 0' }}>
      {steps.map((s) => (
        <div
          key={s.step}
          style={{
            display: 'flex',
            gap: 12,
            alignItems: 'flex-start',
            background: 'var(--panel-2)',
            padding: '10px 14px',
            borderRadius: 8,
            border: '1px solid var(--border)',
          }}
        >
          <div
            style={{
              minWidth: 26,
              height: 26,
              borderRadius: '50%',
              background: 'var(--accent)',
              color: '#fff',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              fontWeight: 'bold',
              fontSize: 12.5,
              marginTop: 1,
            }}
          >
            {s.step}
          </div>
          <div style={{ flex: 1 }}>
            <div style={{ fontWeight: 600, color: 'var(--text)', fontSize: 13.5, marginBottom: 2 }}>{s.title}</div>
            <div style={{ color: 'var(--muted)', fontSize: 13, lineHeight: 1.55 }}>{s.desc}</div>
          </div>
        </div>
      ))}
    </div>
  )
}

function AlertBox({ type, title, children }: { type: 'tip' | 'warning' | 'info'; title?: string; children: ReactNode }) {
  const colors = {
    tip: { bg: 'rgba(63, 191, 111, 0.08)', border: 'rgba(63, 191, 111, 0.3)', icon: '💡', title: 'var(--green)' },
    warning: { bg: 'rgba(230, 184, 79, 0.08)', border: 'rgba(230, 184, 79, 0.3)', icon: '⚠️', title: 'var(--yellow)' },
    info: { bg: 'rgba(79, 140, 255, 0.08)', border: 'rgba(79, 140, 255, 0.3)', icon: 'ℹ️', title: 'var(--accent)' },
  }[type]

  return (
    <div style={{ background: colors.bg, border: `1px solid ${colors.border}`, borderRadius: 8, padding: '10px 14px', margin: '12px 0' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontWeight: 600, color: colors.title, fontSize: 13.5, marginBottom: 4 }}>
        <span>{colors.icon}</span>
        <span>{title || (type === 'tip' ? 'Совет' : type === 'warning' ? 'Важно' : 'Информация')}</span>
      </div>
      <div style={{ color: 'var(--text)', fontSize: 13, lineHeight: 1.55 }}>{children}</div>
    </div>
  )
}

type SectionKey =
  | 'all'
  | 'quickstart'
  | 'failover'
  | 'zapret'
  | 'whitelist'
  | 'gaming'
  | 'adguard'
  | 'troubleshooting'
  | 'devices'
  | 'traffic'
  | 'editor'
  | 'shortcuts'

export default function Help({ status }: Props) {
  const [activeSec, setActiveSec] = useState<SectionKey>('all')
  const [query, setQuery] = useState('')

  const q = query.trim().toLowerCase()

  const shouldShow = (sec: SectionKey, textContent: string) => {
    if (activeSec !== 'all' && activeSec !== sec) return false
    if (!q) return true
    return textContent.toLowerCase().includes(q)
  }

  const sections = useMemo(() => [
    { key: 'all' as SectionKey, label: 'Все разделы' },
    { key: 'quickstart' as SectionKey, label: '🚀 Быстрый старт' },
    { key: 'failover' as SectionKey, label: '🧠 Smart Failover 2.0' },
    { key: 'zapret' as SectionKey, label: '⚡ Zapret (DPI)' },
    { key: 'whitelist' as SectionKey, label: '🛡️ Белые списки' },
    { key: 'gaming' as SectionKey, label: '🎮 Игры & Консоли' },
    { key: 'adguard' as SectionKey, label: '🛡️ AdGuard Home' },
    { key: 'troubleshooting' as SectionKey, label: '❓ Вопросы & Ошибки' },
    { key: 'devices' as SectionKey, label: '📱 Устройства' },
    { key: 'traffic' as SectionKey, label: '📈 График трафика' },
    { key: 'editor' as SectionKey, label: '📝 Редактор' },
    { key: 'shortcuts' as SectionKey, label: '⌨️ Горячие клавиши' },
  ], [])

  return (
    <div style={{ maxWidth: 980, margin: '0 auto' }}>
      {/* Главный заголовок и поисковая панель */}
      <div className="card" style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
          <div>
            <h1 style={{ margin: '0 0 4px', fontSize: 20, color: 'var(--text)' }}>📖 Руководство пользователя XKeen Route</h1>
            <P>
              Понятная и наглядная инструкция по всем возможностям панели без сложного технического жаргона.
              Установленная версия: <K>{status?.version || 'v1.9.60'}</K>.
            </P>
          </div>
          <div style={{ minWidth: 260, flex: 1, maxWidth: 360 }}>
            <input
              type="text"
              className="input"
              style={{ width: '100%', boxSizing: 'border-box' }}
              placeholder="🔍 Быстрый поиск по всем разделам..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {/* Навигационные табы категорий */}
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 14, paddingTop: 12, borderTop: '1px solid var(--border)' }}>
          {sections.map((s) => (
            <button
              key={s.key}
              type="button"
              className={`btn sm ${activeSec === s.key ? 'primary' : 'ghost'}`}
              onClick={() => setActiveSec(s.key)}
            >
              {s.label}
            </button>
          ))}
        </div>
      </div>

      {/* СЕКЦИЯ 1: БЫСТРЫЙ СТАРТ */}
      {shouldShow('quickstart', 'быстрый старт подключиться выбрать сервер забыть подписка qr ссылка как начать прокси роутер') && (
        <SectionCard title="Раздел 1: 🚀 Быстрый старт — как подключиться и всё забыть" icon="🚀" badge="За 2 минуты">
          <P>
            XKeen Route превращает ваш роутер в автономный центр раздельной маршрутизации. Вам больше не нужно включать
            VPN-приложения на каждом телефоне, смарт-телевизоре или компьютере — всё работает прозрачно для всей локальной сети.
          </P>
          <StepList
            steps={[
              {
                step: 1,
                title: 'Загрузите подписку или добавьте сервер',
                desc: (
                  <>
                    Откройте вкладку <b>«🛰 Серверы»</b>. Нажмите <b>«📥 Импорт ссылок»</b> и вставьте ключ/ссылку от провайдера
                    (или нажмите <b>«📦 Подписки»</b> для регулярного автообновления). Все локации и узлы сразу добавятся в общий список.
                  </>
                ),
              },
              {
                step: 2,
                title: 'Проверьте задержку и выберите лучший сервер',
                desc: (
                  <>
                    Нажмите кнопку <b>«🔍 Пинг»</b>. Панель моментально измерит задержку до каждого узла. Зелёный цвет индикатора
                    означает отличный отклик (до 100 мс). Кликните по подходящему серверу (например, Германия, Нидерланды или Финляндия) — он станет основным.
                  </>
                ),
              },
              {
                step: 3,
                title: 'Всё готово: раздельный интернет работает сам!',
                desc: (
                  <>
                    Отечественные сервисы (Госуслуги, Сбер, Т-Банк, Кинопоиск, Яндекс) открываются напрямую на полной скорости тарифа.
                    Заблокированные сайты, YouTube, Instagram и зарубежные сервисы мягко направляются через выбранный зарубежный сервер.
                  </>
                ),
              },
            ]}
          />
          <AlertBox type="tip" title="Принцип «Настроил и забыл»">
            Вы можете закрыть вкладку браузера или выключить компьютер. Роутер продолжает самостоятельно обслуживать сеть 24/7,
            проверять доступность серверов и страховать от любых блокировок.
          </AlertBox>
        </SectionCard>
      )}

      {/* СЕКЦИЯ 2: SMART FAILOVER 2.0 */}
      {shouldShow('failover', 'smart failover 2.0 tier 1 vps tier 2 белые списки порог изоляции порог снятия тспу глушилки сбои резервирование зомби канарейка') && (
        <SectionCard title="Раздел 2: 🧠 Smart Failover 2.0 — почему интернет не упадёт при глушилках" icon="🧠" badge="Авто-защита">
          <P>
            <b>Smart Failover 2.0</b> — это интеллектуальный автопилот панели. Если зарубежные каналы внезапно заблокированы
            провайдером (ТСПУ) или зарубежный хостинг вышел из строя, система мгновенно переключит вас на устойчивый резерв без потери связи.
          </P>
          <FeatureList
            items={[
              {
                name: '🛰 Tier 1 (VPS) — Скоростные зарубежные серверы',
                desc: (
                  <>
                    Это ваши основные быстрые узлы в Европе и по всему миру (Германия, Нидерланды, Швеция, США). Они обеспечивают
                    высокую скорость скачивания, просмотр видео в 4K и доступ к мировым сервисам и нейросетям.
                  </>
                ),
              },
              {
                name: '🛡️ Tier 2 (Белые списки) — Неуязвимый отечественный резерв',
                desc: (
                  <>
                    Защищённые шлюзы (протоколы VLESS Reality, маскирующиеся под разрешённый трафик внутри РФ).
                    Они подключаются автоматически только тогда, когда основные зарубежные каналы Tier 1 заблокированы или недоступны.
                  </>
                ),
              },
              {
                name: '📉 Порог изоляции простыми словами (по умолчанию 80%)',
                desc: (
                  <>
                    Это доля зарубежных серверов, переставших отвечать. Если 8 из 10 ваших серверов Tier 1 внезапно перестали проходить проверку
                    (явный сигнал массовой фильтрации протоколов цензурой), система объявляет тревогу и мгновенно активирует <b>Tier 2 (Белые списки)</b>.
                  </>
                ),
              },
              {
                name: '📈 Порог снятия ТСПУ простыми словами (по умолчанию 45%)',
                desc: (
                  <>
                    Это процент оживших серверов, необходимый для возврата на скоростной маршрут. Как только блокировка прекращается
                    и более половины основных скоростных узлов Tier 1 снова стабильно отвечают, умный алгоритм возвращает сеть на быстрые зарубежные серверы.
                  </>
                ),
              },
              {
                name: '🧟 Авто-карантин «зомби-серверов» и канареечный тест (Canary)',
                desc: (
                  <>
                    Если сервер формально пингуется, но трафик через него не проходит (узел стал «зомби»), панель помещает его в карантин
                    на 15 минут, предотвращая отправку данных в черную дыру.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* СЕКЦИЯ 3: ZAPRET (DPI) */}
      {shouldShow('zapret', 'zapret dpi youtube 4k discord rtc голос обход блокировок без vps nfqws nfqws2 тспу пакеты') && (
        <SectionCard title="Раздел 3: ⚡ Zapret (DPI) — YouTube 4K на скорости провайдера без траты VPS" icon="⚡" badge="DPI Bypass">
          <P>
            <b>Zapret</b> — это передовая технология локального обхода цензуры прямо на процессоре вашего роутера. Она не гонит гигабайты
            через удалённые серверы, а тонко модифицирует первые служебные пакеты соединения, сбивая с толку фильтры провайдера (ТСПУ).
          </P>
          <FeatureList
            items={[
              {
                name: '🎬 YouTube в 4K/8K на честной скорости вашего тарифа',
                desc: (
                  <>
                    Видеопотоки загружаются напрямую с серверов Google внутри страны. Трафик не расходует лимиты зарубежного VPS,
                    а видео стартует мгновенно без буферизации даже в ультра-высоком разрешении.
                  </>
                ),
              },
              {
                name: '🎙️ Голосовой Discord (RTC) без задержек и заиканий',
                desc: (
                  <>
                    Zapret обрабатывает голосовые UDP-пакеты Discord, устраняя вечное зависание на «Подключение к RTC» и сохраняя
                    идеальную слышимость при общении в играх.
                  </>
                ),
              },
              {
                name: '⚙️ Автоматический подбор стратегий (Mini-Blockcheck)',
                desc: (
                  <>
                    У каждого интернет-провайдера используется собственная модификация оборудования DPI. На вкладке <b>«⚡ Zapret»</b> есть кнопка
                    <b>«🚀 Подобрать лучшую стратегию»</b>: роутер проверит популярные методы десинхронизации и включит самый эффективный вариант.
                  </>
                ),
              },
              {
                name: '🚀 Движок Zapret 2 (nfqws2) и обновление в 1 клик',
                desc: (
                  <>
                    Поддерживает как ультракомпактный движок v1 для моделей с небольшим объёмом памяти, так и современный Zapret 2
                    с расширенными возможностями обхода агрессивных блокировок.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* СЕКЦИЯ 4: БЕЛЫЕ СПИСКИ */}
      {shouldShow('whitelist', 'белые списки госуслуги банки отечественные шлюзы рунет исключения direct провайдер звонки вконтакте vkturn csqtt vk id потоки') && (
        <SectionCard title="Раздел 4: 🛡️ Белые списки — как не терять Госуслуги, банки и сервисы РФ" icon="🛡️" badge="Надёжный Рунет">
          <P>
            Многие российские государственные порталы и банковские сервисы блокируют вход с зарубежных IP-адресов.
            Благодаря технологии раздельной маршрутизации XKeen Route ваши важные отечественные ресурсы работают безупречно.
          </P>
          <FeatureList
            items={[
              {
                name: '🏛️ Раздельная маршрутизация (Split-Routing)',
                desc: (
                  <>
                    Все домены зоны <K>.ru</K>, <K>.рф</K>, подсети российских провайдеров и государственные сайты (Госуслуги, Налог, Сбербанк,
                    ВТБ, Т-Банк, Альфа, Маркетплейсы, Яндекс) направляются <b>напрямую через вашего провайдера (Direct)</b>, минуя любые туннели.
                  </>
                ),
              },
              {
                name: '🇷🇺 Мгновенное добавление исключений',
                desc: (
                  <>
                    Если какой-то редкий сайт открывается некорректно, откройте вкладку <b>«🌐 Домены»</b>, введите его адрес и нажмите
                    <b>«Напрямую (Direct)»</b>. Правило применится за доли секунды.
                  </>
                ),
              },
              {
                name: '🔄 Резервные шлюзы Рунета',
                desc: (
                  <>
                    В случае экстремальных внешних сбоев система задействует локальные шлюзы, сохраняя связь с критически важными сервисами РФ.
                  </>
                ),
              },
              {
                name: '📞 Звонки ВКонтакте (VK-туннель csqtt://) — аварийный канал при БС',
                desc: (
                  <>
                    Если мобильный или домашний оператор блокирует обычные VPN и работают только сервисы из белого списка (ВКонтакте), откройте
                    <b> «🛡️ Белые списки → 📞 Звонки ВКонтакте»</b>. Вставьте профиль <K>csqtt://</K> с вашего VPS-сервера (или укажите IP:порт и пароль)
                    и выберите удобный источник звонков: автоматический вход через <b>VK ID (OAuth)</b>, <b>токен из профиля csqtt://</b> или <b>ручные ссылки на звонки</b>.
                  </>
                ),
              },
              {
                name: '⚡ Мощность VK-туннеля (потоки) и безопасность аккаунта',
                desc: (
                  <>
                    По умолчанию используется оптимальная мощность <K>20</K> потоков (от 3 до 100). Чем больше потоков, тем выше стабильность на загрузках,
                    но выше нагрузка на роутер. В любой момент можно переключить источник звонков или выйти из аккаунта ВКонтакте без потери настроек сервера VPS.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* СЕКЦИЯ 5: ИГРОВОЙ РЕЖИМ И КОНСОЛИ */}
      {shouldShow('gaming', 'игровой режим консоли xbox ps5 psn nintendo ping nat fix_xbox_dns.cmd 0x80a40401 dns smartdns игры steam discord voice') && (
        <SectionCard title="Раздел 5: 🎮 Игровой режим и консоли (ПК, Xbox, PS5) без лагов" icon="🎮" badge="Гейминг без задержек">
          <P>
            Сетевые онлайн-игры требуют минимального отклика и стабильного соединения. Вкладка <b>«🎮 Игры»</b> оптимизирует сетевой
            стек для максимальной производительности игровых ПК и консолей.
          </P>
          <FeatureList
            items={[
              {
                name: '🎯 Игровой трафик по кратчайшему пути — минимальный пинг и открытый NAT',
                desc: (
                  <>
                    Игровые пакеты (CS2, Dota 2, Call of Duty, Fortnite, Battlefield, GTA Online, PUBG) маршрутизируются в обход прокси напрямую
                    к серверам разработчиков, исключая паразитные задержки, дропы и высокий джиттер.
                  </>
                ),
              },
              {
                name: '🟢 Устранение ошибки авторизации Xbox (0x80a40401)',
                desc: (
                  <>
                    Консоли Xbox часто блокируют вход в профиль в регионе РФ. Встроенный сервис SmartDNS автоматически подменяет адреса
                    авторизации Microsoft, восстанавливая вход в аккаунт и сетевую игру.
                  </>
                ),
              },
              {
                name: '📥 Настройка Windows ПК в 1 клик через fix_xbox_dns.cmd',
                desc: (
                  <>
                    Нажмите кнопку <b>«📥 Скачать fix_xbox_dns.cmd»</b> на вкладке «🎮 Игры» (или «Xbox DNS»). Запустите скачанный файл от имени
                    Администратора. Скрипт мгновенно настроит безопасные DNS (<K>111.88.96.54</K>, <K>111.88.96.55</K>) и очистит кэш Windows.
                  </>
                ),
              },
              {
                name: '🎮 Быстрая настройка PlayStation 5 и Xbox вручную',
                desc: (
                  <>
                    В сетевых настройках консоли выберите ручную установку DNS: Основной <K>111.88.96.54</K>, Дополнительный <K>111.88.96.55</K>.
                    После этого авторизация и сетевые службы заработают в штатном режиме.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* СЕКЦИЯ 6: ADGUARD HOME */}
      {shouldShow('adguard', 'adguard home реклама блокировка dns баннеры трекеры фильтры родительский контроль вся квартира без плагинов') && (
        <SectionCard title="Раздел 6: 🛡️ AdGuard Home — блокировка рекламы для всей квартиры" icon="🛡️" badge="Чистый интернет">
          <P>
            Интеграция с <b>AdGuard Home</b> позволяет блокировать баннеры, всплывающую рекламу, фишинг и назойливые трекеры
            на всех устройствах домашней сети одновременно — даже на Smart TV и мобильных устройствах без браузерных расширений.
          </P>
          <FeatureList
            items={[
              {
                name: '🚫 Блокировка рекламы на уровне DNS',
                desc: (
                  <>
                    Рекламные запросы отсекаются ещё до того, как страница начнёт загружаться. В результате сайты открываются быстрее,
                    а видео в приложениях не прерывается баннерами.
                  </>
                ),
              },
              {
                name: '📺 Чистые телевизоры и медиаприставки',
                desc: (
                  <>
                    На Smart TV невозможно поставить обычные блокировщики рекламы. AdGuard Home фильтрует трафик на входе в роутер,
                    защищая телевизоры всей семьи.
                  </>
                ),
              },
              {
                name: '⚡ Удобный контроль прямо из панели',
                desc: (
                  <>
                    На вкладке <b>«AdGuard»</b> можно смотреть журнал запросов (Query Log), временно отключать защиту, добавлять
                    пользовательские DNS-правила или обновлять фильтры в один клик.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* СЕКЦИЯ 7: ВОПРОСЫ И ПРОБЛЕМЫ */}
      {shouldShow('troubleshooting', 'частые вопросы проблемы сайт не открывается сброс соединений 1 клик ошибка 418 капча connections cloudflare перезапуск') && (
        <SectionCard title="Раздел 7: ❓ Частые вопросы и решение проблем" icon="❓" badge="Скорая помощь">
          <P>
            Простые решения для распространённых ситуаций:
          </P>
          <FeatureList
            items={[
              {
                name: '🌐 Что делать, если нужный зарубежный сайт перестал открываться?',
                desc: (
                  <>
                    1. Перейдите на вкладку <b>«🛰 Серверы»</b> и переключитесь на другой узел (возможно, текущий сервер попал под фильтр сайта).<br />
                    2. Перейдите на вкладку <b>«🧭 Маршрутизация»</b> или <b>«🌐 Домены»</b> и убедитесь, что домен направлен через <K>PROXY</K>.<br />
                    3. Воспользуйтесь <b>«✨ Каталогом пресетов»</b> для добавления всех сопутствующих поддоменов в 1 клик.
                  </>
                ),
              },
              {
                name: '🔄 Зависшие соединения и сброс в 1 клик',
                desc: (
                  <>
                    После смены сервера браузер может удерживать старые сессии. Откройте вкладку <b>«🔌 Соединения»</b> и нажмите
                    кнопку <b>«Закрыть все»</b>. Роутер разорвёт старые зависшие сессии, и сайт немедленно загрузится через новый сервер.
                  </>
                ),
              },
              {
                name: '☕ Ошибки 418, бесконечные капчи Cloudflare и ограничения хостинга',
                desc: (
                  <>
                    Ошибка <b>418 (I am a teapot)</b> или постоянная проверка «Проверка безопасности...» возникает, когда защита Cloudflare
                    считает IP-адрес сервера подозрительным. Просто переключитесь на другой узел из списка (лучше всего узлы с чистым статусом Google Flow).
                  </>
                ),
              },
              {
                name: '🔄 Полный перезапуск XKeen за 4 секунды',
                desc: (
                  <>
                    Если что-то пошло не так, нажмите иконку обновления (🔄) в верхнем правом углу шапки. Демон и маршрутизация перезапустятся,
                    сохранив всю вашу конфигурацию.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* ДОПОЛНИТЕЛЬНО: УСТРОЙСТВА */}
      {shouldShow('devices', 'устройства devices назначить сервер гаджет телевизор телефон скорость родительский контроль') && (
        <SectionCard title="Управление домашними устройствами" icon="📱" badge="Разделение комнат">
          <P>
            На вкладке <b>«📱 Устройства»</b> представлен полный список компьютеров, телефонов и телевизоров:
          </P>
          <FeatureList
            items={[
              {
                name: '🛰 Персональный сервер для выбранного гаджета',
                desc: 'В колонке Server можно назначить персональный прокси для конкретного ТВ или смартфона, не влияя на остальных жильцов.',
              },
              {
                name: '🚦 Ограничение скорости (Speed Limit)',
                desc: 'Позволяет задать скоростной лимит (10, 30, 100 Мбит/с) гостевым устройствам, чтобы они не забивали общий канал.',
              },
              {
                name: '🛡️ Защитный маркер «you»',
                desc: 'Панель выделяет ваше текущее устройство синим маркером, предотвращая случайное отключение собственного интернета.',
              },
            ]}
          />
        </SectionCard>
      )}

      {/* ДОПОЛНИТЕЛЬНО: ГРАФИК ТРАФИКА */}
      {shouldShow('traffic', 'график трафика direct proxy загрузка отдача скорость kb/s mb/s gb/s') && (
        <SectionCard title="График сетевой активности (Direct vs Proxy)" icon="📈" badge="Мониторинг канала">
          <P>
            Интерактивный график на главной странице отображает сетевые потоки в реальном времени:
          </P>
          <FeatureList
            items={[
              {
                name: '🔵 Прямой поток (Direct)',
                desc: 'Показывает объём данных российских сайтов, банков и торрентов, которые идут напрямую на полной скорости провайдера.',
              },
              {
                name: '🟢 Проксированный поток (Proxy)',
                desc: 'Отображает трафик заблокированных сервисов и зарубежных платформ, проходящий через зашифрованный туннель.',
              },
            ]}
          />
        </SectionCard>
      )}

      {/* ДОПОЛНИТЕЛЬНО: РЕДАКТОР КОНФИГУРАЦИЙ */}
      {shouldShow('editor', 'редактор конфигов yaml json hot reload ctrl s валидация') && (
        <SectionCard title="Встроенный веб-редактор конфигураций" icon="📝" badge="Для опытных">
          <P>
            Прямо в браузере доступен защищённый редактор системных файлов:
          </P>
          <FeatureList
            items={[
              {
                name: '🛡️ Валидация синтаксиса',
                desc: 'Редактор проверяет структуру YAML и JSON перед записью на диск, защищая роутер от опечаток и сбоев.',
              },
              {
                name: '🔄 Мгновенный Hot Reload',
                desc: 'После сохранения ядро сразу применяет новые правила без необходимости ручного перезапуска.',
              },
            ]}
          />
        </SectionCard>
      )}

      {/* ДОПОЛНИТЕЛЬНО: ГОРЯЧИЕ КЛАВИШИ */}
      {shouldShow('shortcuts', 'горячие клавиши shortcuts ctrl k ctrl p alt r ctrl s') && (
        <SectionCard title="Глобальные горячие клавиши" icon="⌨️" badge="Быстрая навигация">
          <FeatureList
            items={[
              { name: '🔍 Ctrl + K (Cmd + K)', desc: 'Быстро перейти к списку серверов и строке поиска.' },
              { name: '⚡ Ctrl + P (Cmd + P)', desc: 'Запустить измерение задержки всех серверов.' },
              { name: '🔄 Alt + R (Shift + R)', desc: 'Обновить системный статус без перезагрузки вкладки.' },
              { name: '💾 Ctrl + S (в редакторе)', desc: 'Сохранить конфигурационный файл с проверкой синтаксиса.' },
            ]}
          />
        </SectionCard>
      )}
    </div>
  )
}
