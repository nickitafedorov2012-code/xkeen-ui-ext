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
  | 'arch'
  | 'dashboard'
  | 'servers'
  | 'editor'
  | 'devices'
  | 'failover'
  | 'domains'
  | 'settings'
  | 'ram'
  | 'logs'
  | 'api'

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
    { key: 'arch' as SectionKey, label: '🏛 Архитектура' },
    { key: 'dashboard' as SectionKey, label: '📊 Дашборд' },
    { key: 'servers' as SectionKey, label: '🛰 Серверы & QR' },
    { key: 'editor' as SectionKey, label: '📝 Редактор' },
    { key: 'devices' as SectionKey, label: '📱 Устройства' },
    { key: 'failover' as SectionKey, label: '⚡ Failover & Telegram' },
    { key: 'domains' as SectionKey, label: '🌐 Домены & Пресеты' },
    { key: 'settings' as SectionKey, label: '⚙️ Настройки & Защита' },
    { key: 'ram' as SectionKey, label: '🧠 Оптимизация RAM' },
    { key: 'logs' as SectionKey, label: '📄 Логи & Syslog' },
    { key: 'api' as SectionKey, label: '🔌 REST API' },
  ], [])

  return (
    <div style={{ maxWidth: 980, margin: '0 auto' }}>
      {/* Главный заголовок и панель быстрого поиска */}
      <div className="card" style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
          <div>
            <h1 style={{ margin: '0 0 4px', fontSize: 20, color: 'var(--text)' }}>📖 Руководство пользователя XKeen Route</h1>
            <P>
              Полная документация по всем функциям, механизмам раздельной маршрутизации, отказоустойчивости и пошаговой настройке.
              Версия: <K>{status?.version || 'v1.2.0'}</K>.
            </P>
          </div>
          <div style={{ minWidth: 260, flex: 1, maxWidth: 360 }}>
            <input
              type="text"
              className="input"
              style={{ width: '100%', boxSizing: 'border-box' }}
              placeholder="🔍 Быстрый поиск по справке..."
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

      {/* 1. АРХИТЕКТУРА И СТЕК */}
      {shouldShow('arch', 'архитектура стек rci mihomo entware clash meta config.json config.yaml split-routing') && (
        <SectionCard title="Архитектура и принцип работы" icon="🏛" badge="Ядро & Стек">
          <P>
            <b>XKeen Route</b> — это высокопроизводительная веб-панель и интеллектуальный фоновый демон-контроллер,
            написанный на <b>Rust</b> для роутеров Keenetic под управлением прошивки <b>KeeneticOS</b> и среды <b>Entware</b>.
            Панель осуществляет управление раздельной маршрутизацией (split-routing) в связке с прокси-ядром <b>Mihomo</b> (Clash Meta).
          </P>
          <FeatureList
            items={[
              {
                name: '🔗 Интеграция с KeeneticOS (RCI API)',
                desc: (
                  <>
                    Общение с роутером осуществляется через локальный сокет RCI (<K>127.0.0.1:79</K>). Панель автоматически авторизуется через токен <K>/opt/etc/xkeen/xkeen.json</K> (или challenge-auth с логином/паролем), считывает системную информацию о модели роутера, нагрузку CPU/RAM, подключённые устройства (IP, MAC, имена, статус онлайн) и мгновенно переключает политики доступа Keenetic без перезагрузки системы.
                  </>
                ),
              },
              {
                name: '⚡ Интеграция с ядром Mihomo (REST API)',
                desc: (
                  <>
                    Взаимодействие с прокси-ядром происходит через локальный интерфейс Clash API (<K>127.0.0.1:9090</K>). Панель переключает активные узлы, замеряет пинг, динамически подключает прокси-провайдеры, управляет правилами и перезагружает конфигурацию (<K>PUT /configs?force=true</K>) без обрыва текущих сессий.
                  </>
                ),
              },
              {
                name: '📁 Ключевые файлы конфигурации',
                desc: (
                  <>
                    <K>/opt/etc/xkeen-route/config.json</K> — конфигурация панели (failover, псевдонимы подписок, привязки устройств, авторизация, Telegram).<br />
                    <K>/opt/etc/mihomo/config.yaml</K> — конфигурационный файл ядра маршрутизации.<br />
                    <K>/opt/etc/xkeen/ipset/ru_exclude_override.lst</K> — список исключений для прямого обхода блокировок сайтов с российскими IP.<br />
                    <K>/opt/backups/xkeen-route/</K> — каталог моментальных резервных копий.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 2. ШАПКА И СИСТЕМНЫЙ МОНИТОР */}
      {shouldShow('dashboard', 'шапка статус монитор cpu ram память xkeen mihomo обновление github версия sparkline') && (
        <SectionCard title="Верхняя панель и системный монитор" icon="📈" badge="Мониторинг ресурсов">
          <P>
            В верхней панели отображается оперативная сводка состояния стека и аппаратных ресурсов роутера в реальном времени:
          </P>
          <FeatureList
            items={[
              {
                name: '🟢 Индикатор состояния сервиса (Status Wave)',
                desc: (
                  <>
                    Отображает текущее состояние демона XKeen Route. В активном режиме светится зелёным с анимированной неоновой синусоидой (кардиограммой активности). Если сервис остановлен — рамка окрашивается в красный цвет, сигнализируя о сбое.
                  </>
                ),
              },
              {
                name: '🧠 Честный суммарный учёт оперативной памяти XKeen',
                desc: (
                  <>
                    Индикатор <b>XKeen: ~57 МБ</b> отображает реальное совокупное потребление физической памяти (RSS) всего стека: процесса веб-панели <K>xkeen-route</K> (~5 МБ) и процесса ядра маршрутизации <K>mihomo</K> (~52 МБ), считываемое напрямую из <K>/proc</K>. Наведение курсора мыши открывает всплывающую подсказку с точной расшифровкой по каждому компоненту и проценту CPU.
                  </>
                ),
              },
              {
                name: '🖥 Аппаратные ресурсы роутера (CPU и RAM)',
                desc: (
                  <>
                    <b>CPU %</b> — мгновенная общая загрузка процессора Keenetic. <b>RAM занято / всего</b> — общий объём системной оперативной памяти роутера и текущий процент утилизации.
                  </>
                ),
              },
              {
                name: '📦 Автоматическое оповещение о новых версиях',
                desc: (
                  <>
                    Встроенный модуль проверки обновлений регулярно опрашивает актуальный номер релиза через CDN jsDelivr и GitHub. При выходе новой версии в шапке загорается зелёный пульсирующий бейдж со ссылкой на релиз и список изменений.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 3. ДАШБОРД */}
      {shouldShow('dashboard', 'дашборд активный сервер пинг sparkline задержка график failover карточка') && (
        <SectionCard title="Дашборд (Dashboard)" icon="📊" badge="Главный экран">
          <P>
            Центральный пульт управления для быстрого обзора ключевых параметров сети:
          </P>
          <FeatureList
            items={[
              {
                name: '🌐 Карточка «Активный сервер»',
                desc: (
                  <>
                    Показывает текущий рабочий прокси-сервер селектора <K>PROXY</K>, бейдж принадлежности к подписке (<K>📦 Название подписки</K>), измеренный пинг и позицию в цепочке приоритетов (<K>ОСН</K> или <K>РЕЗn</K>). Если активен резервный сервер, карточка выводит уведомление <i>(ожидает восстановления)</i>.
                  </>
                ),
              },
              {
                name: '📈 Sparkline-график стабильности соединения',
                desc: (
                  <>
                    Интерактивная диаграмма задержки под активным сервером. Строит кривую по последним ~40 периодическим опросам: зелёный/жёлтый цвет при стабильном канале и яркие красные маркеры при просадках, таймаутах или обрывах связи.
                  </>
                ),
              },
              {
                name: '🛡 Карточка Failover',
                desc: (
                  <>
                    Отображает текущий статус фонового мониторинга (<K>🟢 включён</K> / <K>⚪ выключен</K>), порог переключения по задержке, количество серверов в цепочке приоритетов и время последней выполненной проверки.
                  </>
                ),
              },
              {
                name: '📟 Системные карточки «Роутер» и «Mihomo»',
                desc: (
                  <>
                    Модель оборудования (например, Keenetic Hopper KN-3810 / Titan KN-1811), установленная версия KeeneticOS, статус связи с RCI API, версия ядра Mihomo и рабочий режим.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 4. СЕРВЕРЫ И ПОДПИСКИ + ССЫЛКИ И QR-КОДЫ */}
      {shouldShow('servers', 'серверы подписки proxy-providers провайдеры переименование алиасы пинг игнор qr ссылка генератор импорт share outbound') && (
        <SectionCard title="Серверы, Подписки, Ссылки и QR-коды" icon="🛰" badge="Прокси & Шеринг">
          <P>
            Вкладка «Серверы» позволяет управлять всем пулом прокси-узлов, импортировать новые узлы, делиться ими через QR-коды и настраивать подписки:
          </P>
          <FeatureList
            items={[
              {
                name: '🔄 Автоматический мержинг серверов (100+ узлов)',
                desc: (
                  <>
                    Панель опрашивает не только статическую секцию <K>proxies:</K>, но и эндпоинт внешних провайдеров <K>/providers/proxies</K>. Все серверы из всех ваших подписок объединяются в единый удобный список.
                  </>
                ),
              },
              {
                name: '🔗 Генератор ссылок и QR-кодов (Share Node)',
                desc: (
                  <>
                    Позволяет экспортировать любой сервер из роутера прямо на телефон или другой клиент. Панель генерирует стандартные URI (<K>vless://</K>, <K>vmess://</K>, <K>ss://</K>, <K>trojan://</K>, <K>hysteria2://</K>) с сохранением всех параметров (Reality, SNI, Path, TLS) и строит чистый векторный SVG QR-код.
                  </>
                ),
              },
              {
                name: '🪄 Импорт серверов (Outbound Generator)',
                desc: (
                  <>
                    Кнопка <b>«+ Добавить серверы»</b> позволяет вставить одну или пачку ссылок (<K>vless://</K>, <K>vmess://</K>, <K>ss://</K>, <K>trojan://</K>, <K>hysteria2://</K>). Панель автоматически валидирует параметры и добавляет узлы в конфигурацию ядра.
                  </>
                ),
              },
              {
                name: '✏️ Пользовательские псевдонимы (переименование подписок)',
                desc: (
                  <>
                    Через кнопку <b>📦 Подписки</b> можно задавать понятные человеческие имена провайдерам (например, вместо технического <K>geodema_sub_1</K> написать «Основная», «Европа» или «Запасная»). Названия сохраняются в <K>config.json</K> и не затираются при обновлении ядра.
                  </>
                ),
              },
              {
                name: '🔍 Параллельный замер пинга',
                desc: (
                  <>
                    Кнопка <b>🔍 Пинг</b> запускает параллельный замер задержки через ядро по всем серверам сразу. Цветовая шкала: зелёный (&lt;100 мс), жёлтый (&lt;250 мс), красный (&gt;250 мс) или прочерк при недоступности.
                  </>
                ),
              },
              {
                name: '📌 Избранные серверы (Pinned)',
                desc: (
                  <>
                    Нажатие на звёздочку/булавку закрепляет важные серверы в отдельной верхней секции для быстрого переключения в 1 клик.
                  </>
                ),
              },
              {
                name: '🚫 Игнор-лист серверов',
                desc: (
                  <>
                    Позволяет исключить сбойные, заблокированные или медленные серверы из автоматических балансировочных групп ядра (<K>Fastest</K>, <K>Fallback</K>).
                  </>
                ),
              },
              {
                name: '🛠 Ремонт кодировок (Починить имена)',
                desc: (
                  <>
                    Автоматически устраняет проблему «кракозябр» (mojibake), возникающую при некорректных кодировках UTF-8 / CP1251 у сторонних провайдеров.
                  </>
                ),
              },
            ]}
          />

          <h3 style={{ margin: '16px 0 8px', fontSize: 14.5, color: 'var(--text)' }}>📱 Пошаговая инструкция: Как поделиться сервером по QR-коду</h3>
          <StepList
            steps={[
              {
                step: 1,
                title: 'Нажмите кнопку «🔗 Ссылка» на карточке сервера',
                desc: 'Кнопка доступна как на подробных, так и на компактных и закреплённых карточках серверов.',
              },
              {
                step: 2,
                title: 'Выберите способ экспорта',
                desc: 'Вы можете нажать «📋 Скопировать ссылку» для отправки текста в мессенджер или отсканировать сгенерированный QR-код прямо с экрана.',
              },
              {
                step: 3,
                title: 'Отсканируйте код в мобильном клиенте',
                desc: 'Откройте V2rayNG, Sing-box, FoXray, Streisand, v2rayN или Shadowrocket на смартфоне и нажмите «Сканировать QR-код». Профиль добавится автоматически.',
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 5. ВЕБ-РЕДАКТОР КОНФИГУРАЦИЙ */}
      {shouldShow('editor', 'редактор конфиг config editor yaml json codemirror горячие клавиши валидация hotkeys ctrl s ctrl f') && (
        <SectionCard title="Встроенный веб-редактор конфигураций (Config Editor)" icon="📝" badge="Новое в v1.2.0">
          <P>
            Полноценный веб-редактор файлов конфигурации прямо в браузере, избавляющий от необходимости подключаться по SSH через nano/vi или держать открытой старую панель на 1000 порту:
          </P>
          <FeatureList
            items={[
              {
                name: '⚡ Поддержка ключевых конфигурационных файлов',
                desc: (
                  <>
                    - <K>/opt/etc/mihomo/config.yaml</K> — конфигурация ядра маршрутизации Mihomo.<br />
                    - <K>/opt/etc/xkeen-route/config.json</K> — персональный конфиг панели и системы failover.<br />
                    - <K>/opt/etc/xkeen/ipset/ru_exclude_override.lst</K> — оверрайд-список исключений RU IPSet.<br />
                    - Файлы автоматических резервных копий.
                  </>
                ),
              },
              {
                name: '🛡 Автоматическая валидация синтаксиса',
                desc: (
                  <>
                    Перед сохранением файл проходит строгую валидацию синтаксиса (YAML/JSON). Если в структуре допущена ошибка (например, неверный отступ или незакрытая кавычка), редактор заблокирует запись и подсветит проблемную строку, предотвращая падение прокси-ядра.
                  </>
                ),
              },
              {
                name: '🔄 Бесшовная перезагрузка ядра (Hot Reload)',
                desc: (
                  <>
                    При сохранении <K>config.yaml</K> панель автоматически отправляет сигнал ядру на перезагрузку конфигурации без перезапуска процесса и без разрыва текущих TCP-соединений.
                  </>
                ),
              },
              {
                name: '⌨️ Горячие клавиши (Hotkeys)',
                desc: (
                  <>
                    - <K>Ctrl + S</K> / <K>Cmd + S</K> — быстрое сохранение с валидацией.<br />
                    - <K>Ctrl + F</K> / <K>Cmd + F</K> — быстрый поиск по содержимому файла.<br />
                    - <K>Ctrl + Z</K> / <K>Ctrl + Y</K> — отмена и повтор изменений.
                  </>
                ),
              },
            ]}
          />

          <AlertBox type="tip" title="Совет по редактированию YAML">
            В файлах YAML отступы делаются исключительно <b>пробелами</b> (обычно 2 пробела на уровень), табуляция (<K>Tab</K>) запрещена стандартом YAML. Редактор автоматически преобразует нажатие клавиши Tab в 2 пробела.
          </AlertBox>
        </SectionCard>
      )}

      {/* 6. УСТРОЙСТВА И МАРШРУТИЗАЦИЯ */}
      {shouldShow('devices', 'устройства devices таблица поиск фильтр онлайн офлайн сортировка you скорость политика gear edit batch') && (
        <SectionCard title="Устройства (Devices) и раздельная маршрутизация" icon="📱" badge="Per-Device Routing">
          <P>
            Вкладка «Устройства» предоставляет гранулярный контроль над каждым клиентом локальной сети:
          </P>
          <FeatureList
            items={[
              {
                name: '🔍 Быстрый поиск и фильтрация Online / Offline',
                desc: (
                  <>
                    Строка поиска мгновенно фильтрует таблицу по имени, IP или MAC-адресу. Кнопки-фильтры <b>🟢 Online</b>, <b>⚪ Offline</b> и <b>All</b> позволяют сфокусироваться на нужных устройствах. Офлайн-устройства аккуратно свернуты в аккордеон.
                  </>
                ),
              },
              {
                name: '📊 Октетная сортировка IP-адресов',
                desc: (
                  <>
                    Клик по заголовку колонки <K>IP ⇅</K> упорядочивает список по числовым значениям октетов IPv4 (адрес <K>192.168.2.2</K> всегда идёт раньше <K>192.168.2.118</K>).
                  </>
                ),
              },
              {
                name: '👤 Защитный бейдж «you» (Текущее устройство)',
                desc: (
                  <>
                    Панель определяет IP-клиента, с которого открыт интерфейс, и выделяет его синим бейджем <b>you</b>, защищая от случайного отключения интернета на собственном компьютере.
                  </>
                ),
              },
              {
                name: '⚡ Селекторы политики и скорости прямо в таблице',
                desc: (
                  <>
                    - <b>Policy</b>: моментальное переключение политики KeeneticOS (<K>Основная</K>, <K>Без интернета</K>, кастомные профили).<br />
                    - <b>Speed</b>: ограничение скорости клиента (<K>10 Мбит/с</K>, <K>30 Мбит/с</K>, <K>100 Мбит/с</K> или свой лимит) через RCI API роутера.<br />
                    - <b>Server</b>: назначение персонального прокси-сервера для конкретного устройства.
                  </>
                ),
              },
              {
                name: '⚙️ Индивидуальный Failover для устройства (Кнопка Edit)',
                desc: (
                  <>
                    Нажатие на шестерёнку открывает окно персональной маршрутизации. Вы можете задать для конкретного устройства собственную цепочку серверов (Основной + Резервные) и отдельный порог пинга. Панель создаёт в ядре изолированную группу <K>AUTO-ip</K> и правило <K>SRC-IP-CIDR</K>.
                  </>
                ),
              },
              {
                name: '✨ Плавающая панель массовых действий (Batch Bar)',
                desc: (
                  <>
                    При отметке нескольких устройств чекбоксами внизу экрана всплывает панель для массовой смены политики или назначения общего прокси-сервера в 1 клик.
                  </>
                ),
              },
            ]}
          />

          <h3 style={{ margin: '16px 0 8px', fontSize: 14.5, color: 'var(--text)' }}>🛠 Пошаговая настройка: Как направить устройство через отдельный сервер</h3>
          <StepList
            steps={[
              {
                step: 1,
                title: 'Найдите устройство в списке',
                desc: 'Используйте строку поиска или фильтр 🟢 Online.',
              },
              {
                step: 2,
                title: 'Выберите сервер в колонке «Server»',
                desc: 'В выпадающем списке вместо «Default (PROXY)» выберите нужный сервер (например, узел в Германии или США).',
              },
              {
                step: 3,
                title: 'Примените индивидуальный Failover (опционально)',
                desc: 'Нажмите кнопку ⚙️ (Edit), добавьте резервные серверы на случай падения основного и нажмите «Сохранить». Всё остальное панель настроит в ядре автоматически.',
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 7. FAILOVER И ОПОВЕЩЕНИЯ В TELEGRAM */}
      {shouldShow('failover', 'failover резервирование приоритет порог пинга telegram бот bot token chat id уведомления автовозврат') && (
        <SectionCard title="Отказоустойчивость (Failover) и Telegram" icon="⚡" badge="Автоматика">
          <P>
            Интеллектуальная система автоматического резервирования и оповещения о сбоях:
          </P>
          <FeatureList
            items={[
              {
                name: '🔄 Фоновый мониторинг задержки',
                desc: (
                  <>
                    Фоновый сервис с заданным интервалом (по умолчанию 30 с) проверяет пинг до приоритетных узлов. Если текущий сервер не отвечает или его задержка превышает заданный порог (например, 300 мс), трафик мгновенно переключается на следующий сервер в цепочке.
                  </>
                ),
              },
              {
                name: '📋 Многоуровневая цепочка приоритетов (Priority Chain)',
                desc: (
                  <>
                    Вы можете составить список серверов: <b>Основной (ОСН)</b> → <b>Резерв 1 (РЕЗ1)</b> → <b>Резерв 2 (РЕЗ2)</b> и так далее. Система всегда стремится удерживать соединение на наиболее приоритетном узле.
                  </>
                ),
              },
              {
                name: '↩️ Автоматический возврат (Auto-Restore)',
                desc: (
                  <>
                    Как только основной сервер восстанавливает работоспособность и стабильный пинг, система автоматически переключает трафик обратно на него.
                  </>
                ),
              },
              {
                name: '🔔 Мгновенные оповещения в Telegram',
                desc: (
                  <>
                    При любом переключении на резервный сервер или успешном автовозврате панель отправляет подробное уведомление в Telegram с указанием причины, пинга и нового активного сервера.
                  </>
                ),
              },
            ]}
          />

          <h3 style={{ margin: '16px 0 8px', fontSize: 14.5, color: 'var(--text)' }}>📲 Пошаговая инструкция: Как подключить Telegram-оповещения</h3>
          <StepList
            steps={[
              {
                step: 1,
                title: 'Создайте Telegram-бота',
                desc: (
                  <>
                    Откройте бота <K>@BotFather</K> в Telegram, отправьте команду <K>/newbot</K>, укажите имя и получите <b>API Token</b> (вид: <K>123456789:ABCdefGhI...</K>).
                  </>
                ),
              },
              {
                step: 2,
                title: 'Активируйте бота и узнайте свой Chat ID',
                desc: (
                  <>
                    Перейдите в созданного бота и нажмите <b>/start</b>. Чтобы узнать свой числовой Chat ID, откройте бота <K>@userinfobot</K> или <K>@getmyid_bot</K> и скопируйте значение <K>Id</K>.
                  </>
                ),
              },
              {
                step: 3,
                title: 'Сохраните настройки в панели',
                desc: (
                  <>
                    Перейдите в <b>Настройки</b> → раздел <b>«🔔 Оповещения в Telegram»</b>. Вставьте токен бота и Chat ID, включите тумблер активности и нажмите <b>«🧪 Отправить тест»</b>. После получения тестового сообщения в Telegram нажмите <b>«Сохранить»</b>.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 8. ДОМЕНЫ И КАТАЛОГ ПРЕСЕТОВ */}
      {shouldShow('domains', 'домены cdn ipset ru_exclude_override direct proxy правила блокировки пресеты каталог preset catalog ai chatgpt claude instagram') && (
        <SectionCard title="Домены, CDN и Каталог готовых пресетов" icon="🌐" badge="Smart Domain Routing">
          <P>
            Управление маршрутизацией на уровне доменных имён и готовых баз сервисов:
          </P>
          <FeatureList
            items={[
              {
                name: '✨ Каталог готовых пресетов (Preset Catalog)',
                desc: (
                  <>
                    Библиотека проверенных наборов правил по категориям с возможностью добавления в 1 клик:<br />
                    - 🤖 <b>AI & Нейросети</b>: ChatGPT, Claude, Midjourney, Google Gemini / AI Studio, Perplexity, Copilot.<br />
                    - 📱 <b>Социальные сети & Мессенджеры</b>: Instagram, Facebook, X (Twitter), Discord, Threads.<br />
                    - 🎬 <b>Стриминг & Видео</b>: YouTube, Netflix, Spotify, Twitch, Deezer.<br />
                    - 🎮 <b>Гейминг & Магазины</b>: Steam Community, PlayStation Network, Epic Games, Xbox Live.<br />
                    - 🛠 <b>IT & Разработка</b>: GitHub, GitLab, Docker Hub, npm, PyPI, Medium.
                  </>
                ),
              },
              {
                name: '⏭ Напрямую (мимо прокси → DIRECT)',
                desc: (
                  <>
                    Домены, направляемые напрямую через шлюз провайдера (<K>DOMAIN-SUFFIX,site.com,DIRECT</K>). Идеально для банковских приложений, Госуслуг и локальных ресурсов.
                  </>
                ),
              },
              {
                name: '🔒 Принудительно через прокси (→ PROXY)',
                desc: (
                  <>
                    Домены, трафик которых всегда принудительно заворачивается в прокси-канал (<K>DOMAIN-SUFFIX,site.com,PROXY</K>).
                  </>
                ),
              },
              {
                name: '⚡ Автоматическое обнаружение CDN и медиа-серверов',
                desc: (
                  <>
                    При добавлении домена алгоритм автоматически находит и подключает связанные поддомены статики (<K>img.</K>, <K>static.</K>, <K>assets.</K>, <K>art.</K>, <K>ext.</K>), предотвращая проблемы незагружающихся картинок и плееров.
                  </>
                ),
              },
              {
                name: '🇷🇺 Интеграция с ipset обхода блокировок RU-зоны',
                desc: (
                  <>
                    Панель автоматически разрешает домены в IP-адреса, записывает их в <K>/opt/etc/xkeen/ipset/ru_exclude_override.lst</K> и атомарно применяет в ipset ядра (<K>geo_override</K>) через команду <K>ipset swap</K>.
                  </>
                ),
              },
            ]}
          />

          <h3 style={{ margin: '16px 0 8px', fontSize: 14.5, color: 'var(--text)' }}>🪄 Как применить готовый пресет за 2 секунды</h3>
          <StepList
            steps={[
              {
                step: 1,
                title: 'Откройте вкладку «Домены» и нажмите «✨ Каталог пресетов»',
                desc: 'Откроется модальное окно со структурированным каталогом категорий.',
              },
              {
                step: 2,
                title: 'Выберите нужный сервис и нажмите «Применить пресет»',
                desc: 'Все необходимые домены, поддомены статики и CDN будут добавлены в список правил маршрутизации ядра.',
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 9. НАСТРОЙКИ, БЕЗОПАСНОСТЬ И СТОРОЖ */}
      {shouldShow('settings', 'настройки бэкапы watchdog сторож безопасность пароль auth speedtest тема light dark') && (
        <SectionCard title="Настройки, Безопасность и Сторож ядра" icon="⚙️" badge="Администрирование">
          <P>
            Управление параметрами безопасности панели, защитой доступа, стабильностью ядра и резервными копиями:
          </P>
          <FeatureList
            items={[
              {
                name: '🔐 Авторизация и защита паролем (Auth & Sessions)',
                desc: (
                  <>
                    Защищает веб-панель от несанкционированного доступа в локальной сети. Пароль хэшируется с криптографической солью (SHA-256 + Salt), а сессионные cookie выдаются на 30 дней с защитой от подделки.
                  </>
                ),
              },
              {
                name: '🛡️ Сторожевой таймер ядра (Watchdog & Auto-healing)',
                desc: (
                  <>
                    Фоновый супервизор проверяет жизнеспособность процесса <K>mihomo</K>. Если ядро аварийно завершилось или зависло, сторож автоматически перезапускает его и гарантирует целостность правил <K>AUTO-DEVICE</K>, <K>AUTO-FORCE</K> и <K>AUTO-IGNORE</K>.
                  </>
                ),
              },
              {
                name: '🎨 Переключение светлой и тёмной темы (Dark / Light)',
                desc: (
                  <>
                    Полноценная поддержка тёмной и светлой темы оформления с синхронизированной цветовой палитрой и высокой контрастностью текста.
                  </>
                ),
              },
              {
                name: '🚀 Встроенный Speedtest',
                desc: (
                  <>
                    Позволяет измерить реальную скорость загрузки и пропускную способность выбранного прокси-сервера путём прямой загрузки тестового пакета через сокет ядра.
                  </>
                ),
              },
              {
                name: '💾 Резервные копии конфигурации (Backups)',
                desc: (
                  <>
                    Создание моментальных снимков <K>config.yaml</K> и <K>config.json</K> в каталоге <K>/opt/backups/xkeen-route/</K> с возможностью восстановления в 1 клик.
                  </>
                ),
              },
            ]}
          />

          <AlertBox type="warning" title="Экстренный сброс пароля панели">
            Если вы забыли установленный пароль администратора веб-панели, подключитесь к роутеру по SSH и выполните команду:
            <div style={{ marginTop: 6 }}><K>/opt/bin/xkeen-route reset-password</K></div>
            Пароль будет мгновенно сброшен, и панель откроется без запроса авторизации.
          </AlertBox>
        </SectionCard>
      )}

      {/* 10. ОПТИМИЗАЦИЯ ОПЕРАТИВНОЙ ПАМЯТИ */}
      {shouldShow('ram', 'память ram оптимизация озу gomemlimit gogc memconservative xkeen-ui 1000 утечки') && (
        <SectionCard title="Оптимизация оперативной памяти (RAM) и отключение XKeen-UI" icon="🧠" badge="Производительность">
          <P>
            Роутеры Keenetic часто имеют ограниченный объём оперативной памяти (от 128 до 512 МБ). Для максимальной стабильности системы применён комплекс мер:
          </P>
          <FeatureList
            items={[
              {
                name: '⚡ Тюнинг рантайма Go (GOMEMLIMIT & GOGC)',
                desc: (
                  <>
                    В init-скрипт <K>/opt/etc/init.d/S05xkeen</K> добавлены флаги сборщика мусора Go:<br />
                    - <K>GOMEMLIMIT=96MiB</K> — строгий мягкий лимит памяти для процесса Go.<br />
                    - <K>GOGC=30</K> — более частый и агрессивный сбор неиспользуемых структур данных.<br />
                    - <K>GODEBUG=madvdontneed=1</K> — немедленный возврат освобождённых страниц памяти ядру Linux.
                  </>
                ),
              },
              {
                name: '📦 Режим экономии памяти Mihomo (memconservative)',
                desc: (
                  <>
                    В <K>/opt/etc/mihomo/config.yaml</K> включена опция <K>geodata-loader: memconservative</K> и отключено избыточное кэширование Fake-IP на накопитель. Это снижает потребление RAM ядром со 196 МБ до <b>~52 МБ</b> (экономия ~70%).
                  </>
                ),
              },
              {
                name: '🛑 Полное отключение устаревшего XKeen-UI (порт 1000)',
                desc: (
                  <>
                    Поскольку XKeen Route содержит в себе весь функционал (включая редактор конфигураций, генераторы ссылок, каталог пресетов и мониторинг), старый интерфейс на порту 1000 больше не требуется и отключён (<K>ENABLED=no</K> в <K>/opt/etc/init.d/S99xkeen-ui</K>), что освобождает память и ресурсы процессора.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 11. ЛОГИ И SYSLOG */}
      {shouldShow('logs', 'логи журнал syslog websocket live ротация скачать') && (
        <SectionCard title="Системный журнал (Логи) и Live-поток" icon="📄" badge="Диагностика">
          <P>
            Мониторинг всех происходящих в системе событий:
          </P>
          <FeatureList
            items={[
              {
                name: '📄 Системный файл журнала',
                desc: (
                  <>
                    Все события (переключения серверов, применение правил, сбои, фоновые проверки failover, HTTP-запросы) пишутся в <K>/opt/etc/xkeen-route/xkeen-route.log</K>. Предусмотрена автоматическая ротация при размере 2 МБ (с сохранением резервной копии <K>.log.old</K>).
                  </>
                ),
              },
              {
                name: '🔴 Режим Live (WebSocket)',
                desc: (
                  <>
                    Вкладка журнала поддерживает режим живой трансляции: браузер устанавливает WebSocket-соединение с <K>/api/logs/ws</K>, и новые строки появляются на экране моментально без нагрузки на процессор роутера.
                  </>
                ),
              },
              {
                name: '📡 Удалённый Syslog (RFC 3164)',
                desc: (
                  <>
                    В настройках можно указать адрес внешнего сервера syslog (<K>host:port</K> по UDP). Все записи панели будут дублироваться во внешнюю систему сбора логов (Graylog, Loki, Grafana, Syslog-ng).
                  </>
                ),
              },
              {
                name: '⬇ Скачивание и очистка',
                desc: (
                  <>
                    Кнопка «Скачать» отдаёт полный файл журнала на компьютер пользователя, а «Очистить» безопасно освобождает место на накопителе роутера.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 12. REST API */}
      {shouldShow('api', 'api rest curl скрипты статус серверы эндпоинты json websocket') && (
        <SectionCard title="REST API для внешних скриптов и интеграций" icon="🔌" badge="Интеграция">
          <P>
            Панель предоставляет полноценный REST API с возвратом данных в формате JSON:
          </P>
          <FeatureList
            items={[
              {
                name: 'GET /api/status',
                desc: 'Сводная информация: модель роутера, версия панели, активный сервер, пинг, загрузка CPU, общая память и суммарная память XKeen.',
              },
              {
                name: 'GET /api/servers & POST /api/servers/switch',
                desc: 'Получение списка всех серверов с провайдерами и пингом; моментальное переключение активного узла (тело: {"server_id": "..."}).',
              },
              {
                name: 'GET /api/providers & POST /api/providers/rename',
                desc: 'Список подключённых подписок провайдеров и сохранение пользовательских названий подписок.',
              },
              {
                name: 'GET /api/devices & POST /api/devices/policy & POST /api/devices/speed',
                desc: 'Список клиентов сети с их IP/MAC; массовое изменение политик и лимитов скорости.',
              },
              {
                name: 'GET /api/routing & POST /api/routing',
                desc: 'Управление индивидуальной привязкой серверов и цепочек failover к IP-адресам устройств.',
              },
              {
                name: 'GET /api/config/files & GET /api/config/read & POST /api/config/write',
                desc: 'Чтение и запись конфигурационных файлов с валидацией синтаксиса и hot-reload ядра.',
              },
              {
                name: 'POST /api/failover/check',
                desc: 'Принудительный запуск цикла проверки отказоустойчивости с возвратом диагностического отчёта.',
              },
              {
                name: 'GET /api/logs/ws',
                desc: 'WebSocket-эндпоинт для потокового получения новых строк системного журнала в реальном времени.',
              },
            ]}
          />
        </SectionCard>
      )}
    </div>
  )
}
