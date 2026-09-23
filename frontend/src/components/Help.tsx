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
  | 'shortcuts'
  | 'arch'
  | 'dashboard'
  | 'servers'
  | 'editor'
  | 'devices'
  | 'google-ai'
  | 'failover'
  | 'domains'
  | 'settings'
  | 'dns'
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
    { key: 'shortcuts' as SectionKey, label: '⌨️ Горячие клавиши' },
    { key: 'arch' as SectionKey, label: '🏛 Архитектура' },
    { key: 'dashboard' as SectionKey, label: '📊 Дашборд & Шапка' },
    { key: 'servers' as SectionKey, label: '🛰 Серверы & QR' },
    { key: 'editor' as SectionKey, label: '📝 Редактор конфигов' },
    { key: 'devices' as SectionKey, label: '📱 Устройства' },
    { key: 'google-ai' as SectionKey, label: '🤖 Google AI & Proxy' },
    { key: 'failover' as SectionKey, label: '⚡ Failover & Telegram' },
    { key: 'domains' as SectionKey, label: '🌐 Домены & Пресеты' },
    { key: 'settings' as SectionKey, label: '⚙️ Настройки & Бэкапы' },
    { key: 'dns' as SectionKey, label: '🧭 DNS (Fake-IP/Redir)' },
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
            <h1 style={{ margin: '0 0 4px', fontSize: 20, color: 'var(--text)' }}>📖 Полное руководство пользователя XKeen Route</h1>
            <P>
              Исчерпывающая справка по всем функциям, кнопкам, механизмам маршрутизации, горячим клавишам и пошаговой настройке.
              Текущая версия: <K>{status?.version || 'v1.2.4'}</K>.
            </P>
          </div>
          <div style={{ minWidth: 260, flex: 1, maxWidth: 360 }}>
            <input
              type="text"
              className="input"
              style={{ width: '100%', boxSizing: 'border-box' }}
              placeholder="🔍 Быстрый поиск по всей справке..."
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

      {/* 1. ГОРЯЧИЕ КЛАВИШИ */}
      {shouldShow('shortcuts', 'горячие клавиши hotkeys shortcuts ctrl k ctrl p alt r ctrl s ctrl f быстрые клавиши') && (
        <SectionCard title="Глобальные горячие клавиши (Keyboard Shortcuts)" icon="⌨️" badge="Быстрая навигация">
          <P>
            В веб-панели реализованы глобальные клавиатурные сочетания для мгновенной навигации без использования мыши:
          </P>
          <FeatureList
            items={[
              {
                name: '🔍 Ctrl + K (или Cmd + K)',
                desc: (
                  <>
                    Мгновенно открывает вкладку <b>«🛰 Серверы»</b> и устанавливает фокус в строку поиска. Позволяет сразу начать ввод названия страны, города или протокола.
                  </>
                ),
              },
              {
                name: '⚡ Ctrl + P (или Cmd + P)',
                desc: (
                  <>
                    Переходит на вкладку <b>«🛰 Серверы»</b> и немедленно запускает параллельный замер пинга всех доступных серверов.
                  </>
                ),
              },
              {
                name: '🔄 Alt + R (или Shift + R)',
                desc: (
                  <>
                    Принудительно обновляет данные системного монитора, статус роутера и текущий активный сервер без полной перезагрузки веб-страницы.
                  </>
                ),
              },
              {
                name: '📝 Ctrl + S (в редакторе конфигураций)',
                desc: (
                  <>
                    Запускает валидацию синтаксиса и сохраняет открытый файл на роутер с мгновенным hot-reload ядра.
                  </>
                ),
              },
              {
                name: '🔎 Ctrl + F (в редакторе конфигураций)',
                desc: (
                  <>
                    Открывает панель быстрого поиска и замены внутри редактируемого конфигурационного файла.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 2. АРХИТЕКТУРА И СТЕК */}
      {shouldShow('arch', 'архитектура стек rci mihomo entware clash meta config.json config.yaml split-routing') && (
        <SectionCard title="Архитектура и принцип работы" icon="🏛" badge="Ядро & Стек">
          <P>
            <b>XKeen Route</b> — это специализированный высокопроизводительный демон-контроллер и веб-панель,
            написанный на <b>Rust</b> для роутеров Keenetic под управлением прошивки <b>KeeneticOS</b> и среды <b>Entware</b>.
            Панель осуществляет управление раздельной маршрутизацией (split-routing) в связке с прокси-ядром <b>Mihomo</b> (Clash Meta).
          </P>
          <FeatureList
            items={[
              {
                name: '🔗 Интеграция с KeeneticOS (RCI API)',
                desc: (
                  <>
                    Общение с роутером осуществляется через локальный сокет RCI (<K>127.0.0.1:79</K>). Панель автоматически считывает токен из <K>/opt/etc/xkeen/xkeen.json</K> (или challenge-auth с логином/паролем), получает список клиентов (IP, MAC, имена, статус онлайн), нагрузку процессора/памяти и мгновенно переключает политики доступа без перезагрузки системы.
                  </>
                ),
              },
              {
                name: '⚡ Интеграция с ядром Mihomo (Clash REST API)',
                desc: (
                  <>
                    Взаимодействие с прокси-ядром происходит через Clash API (<K>127.0.0.1:9090</K>). Панель переключает активные узлы, замеряет пинг, динамически подключает прокси-провайдеры, управляет правилами и перезагружает конфигурацию (<K>PUT /configs?force=true</K>) без обрыва текущих соединений.
                  </>
                ),
              },
              {
                name: '📁 Ключевые файлы конфигурации',
                desc: (
                  <>
                    <K>/opt/etc/xkeen-route/config.json</K> — персональный конфиг панели (failover, псевдонимы подписок, привязки устройств, авторизация, Telegram).<br />
                    <K>/opt/etc/mihomo/config.yaml</K> — конфигурационный файл ядра маршрутизации.<br />
                    <K>/opt/etc/xkeen/ipset/ru_exclude_override.lst</K> — список оверрайдов для прямого обхода блокировок сайтов с российскими IP.<br />
                    <K>/opt/backups/xkeen-route/</K> — каталог моментальных резервных копий.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 3. ШАПКА И СИСТЕМНЫЙ МОНИТОР */}
      {shouldShow('dashboard', 'шапка статус монитор cpu ram память xkeen mihomo обновление github версия sparkline кардиограмма') && (
        <SectionCard title="Верхняя панель и системный монитор" icon="📈" badge="Мониторинг ресурсов">
          <P>
            В верхней панели отображается оперативная сводка состояния стека и аппаратных ресурсов роутера в реальном времени:
          </P>
          <FeatureList
            items={[
              {
                name: '🟢 Индикатор состояния сервиса (Status Waveform)',
                desc: (
                  <>
                    Отображает текущее состояние демона XKeen Route. В активном режиме светится зелёным с анимированной неоновой кардиограммой. Если сервис остановлен — рамка окрашивается в красный цвет. Кнопки рядом позволяют перезапустить failover (🔄) или остановить/запустить службу (▶ / ⏹).
                  </>
                ),
              },
              {
                name: '🧠 Честный суммарный учёт оперативной памяти XKeen',
                desc: (
                  <>
                    Индикатор <b>XKeen: ~52-57 МБ</b> отображает реальное совокупное потребление физической памяти (RSS) всего стека: процесса веб-панели <K>xkeen-route</K> (~5 МБ) и процесса ядра маршрутизации <K>mihomo</K> (~52 МБ), считываемое напрямую из <K>/proc</K>. Наведение курсора открывает всплывающую подсказку с точной расшифровкой.
                  </>
                ),
              },
              {
                name: '🖥 Аппаратные ресурсы роутера (CPU и RAM)',
                desc: (
                  <>
                    <b>RAM</b> — занятый и общий объём памяти роутера с цветовым мини-баром. <b>CPU %</b> — мгновенная общая загрузка процессора роутера.
                  </>
                ),
              },
              {
                name: '📦 Чипы Mihomo и Версии панели',
                desc: (
                  <>
                    Клик по чипу <b>Mihomo</b> мгновенно переносит на вкладку серверов. Чип версии панели подсвечивается зелёным пульсирующим бейджем при выходе нового релиза на GitHub.
                  </>
                ),
              },
              {
                name: '🌙 / ☀️ Переключатель темы (Dark / Light)',
                desc: (
                  <>
                    Кнопка в шапке переключает интерфейс между тёмной и светлой темой с адаптацией всех карточек, графиков и редактора кода.
                  </>
                ),
              },
              {
                name: '📝 Кнопка быстрого открытия редактора',
                desc: (
                  <>
                    Иконка блокнота в шапке открывает модальное окно веб-редактора конфигов из любого места панели.
                  </>
                ),
              },
              {
                name: '🚪 Кнопка выхода (Logout)',
                desc: (
                  <>
                    Отображается при включённой защите паролем и позволяет завершить текущую сессию администратора.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 4. ДАШБОРД */}
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
                    Показывает текущий рабочий узел селектора <K>PROXY</K>, бейдж подписки (<K>📦 Название</K>), измеренный пинг и позицию в цепочке приоритетов (<K>ОСН</K> или <K>РЕЗn</K>). Если активен резервный узел, выводится статус <i>(ожидает восстановления)</i>.
                  </>
                ),
              },
              {
                name: '📈 Sparkline-график стабильности соединения',
                desc: (
                  <>
                    Интерактивная диаграмма задержки под активным сервером. Строит кривую по последним ~40 периодическим опросам: зелёный/жёлтый цвет при стабильном канале и красные маркеры при просадках или таймаутах.
                  </>
                ),
              },
              {
                name: '🛡 Карточка Failover',
                desc: (
                  <>
                    Отображает текущий статус фонового мониторинга (<K>🟢 включён</K> / <K>⚪ выключен</K>), порог переключения по задержке, количество серверов в цепочке приоритетов и время последней проверки.
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

      {/* 5. СЕРВЕРЫ И ПОДПИСКИ + ССЫЛКИ И QR-КОДЫ */}
      {shouldShow('servers', 'серверы подписки proxy-providers провайдеры переименование алиасы пинг игнор qr ссылка генератор импорт share outbound избранное pinned') && (
        <SectionCard title="Серверы, Подписки, Ссылки и QR-коды" icon="🛰" badge="Прокси & Шеринг">
          <P>
            Вкладка «Серверы» позволяет управлять всем пулом узлов, импортировать новые узлы, делиться ими через QR-коды и настраивать внешние подписки:
          </P>
          <FeatureList
            items={[
              {
                name: '🔄 Автоматический мержинг серверов (100+ узлов)',
                desc: (
                  <>
                    Панель опрашивает статическую секцию <K>proxies:</K> и эндпоинт внешних провайдеров <K>/providers/proxies</K>. Все серверы из всех ваших подписок объединяются в единый удобный список.
                  </>
                ),
              },
              {
                name: '🔗 Генератор ссылок и QR-кодов (Share Node)',
                desc: (
                  <>
                    Кнопка <b>«🔗 Ссылка»</b> на карточке любого сервера генерирует стандартные URI (<K>vless://</K>, <K>vmess://</K>, <K>ss://</K>, <K>trojan://</K>, <K>hysteria2://</K>) с сохранением всех параметров (Reality, SNI, Path, TLS) и строит чистый векторный SVG QR-код для мгновенного сканирования мобильными клиентами.
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
                    Нажатие на иконку булавки закрепляет важные серверы в отдельной верхней секции для быстрого переключения в 1 клик.
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
                desc: 'Вы можете нажать «📋 Скопировать ссылку» для отправки текста в буфер или отсканировать сгенерированный QR-код прямо с экрана.',
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

      {/* 6. ВЕБ-РЕДАКТОР КОНФИГУРАЦИЙ */}
      {shouldShow('editor', 'редактор конфиг config editor yaml json codemirror горячие клавиши валидация hotkeys ctrl s ctrl f') && (
        <SectionCard title="Встроенный веб-редактор конфигураций (Config Editor)" icon="📝" badge="Редактирование">
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

      {/* 7. УСТРОЙСТВА И МАРШРУТИЗАЦИЯ */}
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

      {/* 8. GOOGLE AI И ANTIGRAVITY */}
      {shouldShow('google-ai', 'google ai antigravity gemini flow studio ai studio proxy 53129 socks http proxy env переменные') && (
        <SectionCard title="Google AI, Gemini Labs и служба Antigravity" icon="🤖" badge="AI Acceleration">
          <P>
            Специализированный модуль для гарантированного и стабильного доступа к сервисам искусственного интеллекта Google (Google AI Studio, Gemini API, Google Flow, Vertex AI):
          </P>
          <FeatureList
            items={[
              {
                name: '🔍 Автоматическая проверка чистоты гео-позиции (Google Flow Check)',
                desc: (
                  <>
                    Кнопка <b>«🔍 Проверить Google Flow»</b> тестирует текущий активный сервер на предмет связывания поисковой системой Google с регионами РФ/СНГ. Показывает точный регион Google (<K>google_country</K>) и домен выдачи (<K>google.com</K> vs <K>google.ru</K>).
                  </>
                ),
              },
              {
                name: '⚡ Селектор чистых AI-узлов (Google AI Nodes)',
                desc: (
                  <>
                    Выпадающий список фильтрует серверы, оставляя только гарантированно чистые узлы в США, Великобритании и Канаде, и позволяет переключить AI-маршрут в 1 клик.
                  </>
                ),
              },
              {
                name: '🛡 Служба Antigravity и авто-подстановка DNS',
                desc: (
                  <>
                    Фоновая служба регулярно проверяет доступность IP-адресов Google API и при обнаружении блокировок автоматически подставляет рабочие незаблокированные DNS-записи.
                  </>
                ),
              },
              {
                name: '🔌 Выделенный локальный порт прокси (53129)',
                desc: (
                  <>
                    Панель поднимает отдельный локальный прокси-порт (по умолчанию <K>192.168.2.1:53129</K>), направляющий весь трафик через проверенный чистый AI-канал.
                  </>
                ),
              },
              {
                name: '📋 Готовые команды экспорта переменных окружения',
                desc: (
                  <>
                    Блок быстрых команд позволяет скопировать строки экспорта для <b>Bash/Zsh</b> (<K>export HTTP_PROXY=...</K>), <b>Windows CMD</b> (<K>set HTTP_PROXY=...</K>) и <b>PowerShell</b> (<K>$env:HTTP_PROXY=...</K>) для подключения Antigravity IDE, Python SDK, curl и git к локальному AI-порту роутера.
                  </>
                ),
              },
            ]}
          />

          <h3 style={{ margin: '16px 0 8px', fontSize: 14.5, color: 'var(--text)' }}>🚀 Как подключить консоль / IDE к прокси роутера для Google AI</h3>
          <StepList
            steps={[
              {
                step: 1,
                title: 'Откройте вкладку «🤖 Google AI»',
                desc: 'Убедитесь, что служба Antigravity включена и активен чистый узел (США или UK).',
              },
              {
                step: 2,
                title: 'Скопируйте команду для вашей операционной системы',
                desc: 'Нажмите кнопку копирования рядом с нужной командной оболочкой (PowerShell, CMD или Bash).',
              },
              {
                step: 3,
                title: 'Вставьте команду в терминал',
                desc: 'Все последующие запросы Python SDK, curl, npm и git в этом терминале пойдут через защищённый канал роутера.',
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 9. FAILOVER И ОПОВЕЩЕНИЯ В TELEGRAM */}
      {shouldShow('failover', 'failover резервирование приоритет порог пинга telegram бот bot token chat id уведомления автовозврат webhook') && (
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
                    Фоновый сервис с заданным интервалом (по умолчанию 30 с) проверяет пинг до приоритетных узлов. Если текущий сервер не отвечает или его задержка превышает заданный порог (например, 300 мс), трафик мгновенно переключается на следующий рабочий сервер в цепочке.
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
                name: '🔔 Мгновенные оповещения в Telegram и Webhook',
                desc: (
                  <>
                    При любом переключении на резервный сервер или успешном автовозврате панель отправляет подробное уведомление в Telegram (и на внешний Webhook URL при его наличии) с указанием причины, пинга и нового активного сервера.
                  </>
                ),
              },
              {
                name: '💾 Мгновенное автосохранение',
                desc: (
                  <>
                    Любые изменения в цепочке приоритетов или порогах задержки автоматически сохраняются в конфигурацию без необходимости нажатия дополнительных кнопок.
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
                    Откройте бота <K>@BotFather</K> в Telegram, отправьте команду <K>/newbot</K>, укажите имя и скопируйте <b>API Token</b> (вид: <K>123456789:ABCdefGhI...</K>).
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
                    Перейдите в <b>Настройки</b> → раздел <b>«🔔 Оповещения о сбоях»</b>. Вставьте токен бота и Chat ID, включите тумблер активности и нажмите <b>«💬 Тестовое сообщение»</b>. После получения сообщения в Telegram нажмите <b>«💾 Сохранить оповещения»</b>.
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 10. ДОМЕНЫ И КАТАЛОГ ПРЕСЕТОВ */}
      {shouldShow('domains', 'домены cdn ipset ru_exclude_override direct proxy правила блокировки пресеты каталог preset catalog ai chatgpt claude instagram youtube discord') && (
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
                title: 'Откройте раздел «Домены» в Настройках и нажмите «✨ Каталог пресетов»',
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

      {/* 11. НАСТРОЙКИ, БЕЗОПАСНОСТЬ И СТОРОЖ */}
      {shouldShow('settings', 'настройки бэкапы watchdog сторож безопасность пароль auth speedtest обновление update xkbak rci mihomo') && (
        <SectionCard title="Настройки, Безопасность, Бэкапы и Обновление" icon="⚙️" badge="Администрирование">
          <P>
            Управление параметрами безопасности панели, защитой доступа, стабильностью ядра, резервными копиями и обновлениями:
          </P>
          <FeatureList
            items={[
              {
                name: '🔐 Авторизация и защита паролем (Auth & Sessions)',
                desc: (
                  <>
                    Защищает веб-панель от несанкционированного доступа в локальной сети. Пароль хэшируется с солью (SHA-256 + Salt), а сессионные cookie выдаются на 30 дней с защитой от подделки.
                  </>
                ),
              },
              {
                name: '🛡️ Сторожевой таймер ядра (Watchdog & Auto-healing)',
                desc: (
                  <>
                    Фоновый супервизор демона каждые 4 секунды проверяет конфигурацию <K>config.yaml</K>. При сбоях или ручных перезапусках XKeen сторож автоматически восстанавливает правила <K>AUTO-DEVICE</K>, <K>AUTO-FORCE</K> и <K>AUTO-IGNORE</K> без обрыва связи.
                  </>
                ),
              },
              {
                name: '💾 Резервные копии конфигурации (.xkbak)',
                desc: (
                  <>
                    Создание моментальных снимков <K>config.yaml</K> и <K>config.json</K> в каталоге <K>/opt/backups/xkeen-route/</K>. Доступны функции восстановления в 1 клик, скачивания архива на компьютер и <b>загрузки файла бэкапа с ПК (📤 Загрузить архив)</b>.
                  </>
                ),
              },
              {
                name: '🔄 Автоматическое обновление панели в 1 клик',
                desc: (
                  <>
                    В разделе настроек можно проверить наличие свежей версии на GitHub, просмотреть список изменений («Что нового») и нажать кнопку <b>«Обновить»</b> — панель сама скачает бинарный файл, заменит его и перезапустит сервис за 8 секунд.
                  </>
                ),
              },
              {
                name: '🖥 Управление сервисом XKeen',
                desc: (
                  <>
                    Кнопки <b>Статус</b>, <b>Старт</b>, <b>Рестарт</b> и <b>Стоп</b> для системного скрипта <K>/opt/etc/init.d/S05xkeen</K>.
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

      {/* 12. DNS РЕЖИМЫ (FAKE-IP VS REDIR-HOST) */}
      {shouldShow('dns', 'dns режим fake-ip redir-host fake ip redir host резолв keenetic home lan') && (
        <SectionCard title="DNS-режимы ядра (Fake-IP vs Redir-Host)" icon="🧭" badge="Сетевой стек">
          <P>
            Выбор режима обработки DNS-запросов прокси-ядром Mihomo:
          </P>
          <FeatureList
            items={[
              {
                name: '⚡ Fake-IP (Рекомендуется по умолчанию)',
                desc: (
                  <>
                    <b>Как работает</b>: ядро мгновенно возвращает клиенту виртуальный IP-адрес из диапазона <K>198.18.0.0/16</K> (~1 мс отклика) без ожидания ответа удалённых DNS-серверов. Реальный DNS-резолв выполняется удалённым прокси-сервером.<br />
                    <b>Плюсы</b>: нулевая задержка DNS, идеальный обход DPI и блокировок, максимальная скорость в браузере, играх и мессенджерах.
                  </>
                ),
              },
              {
                name: '🌐 Redir-Host (Прямой резолв)',
                desc: (
                  <>
                    <b>Как работает</b>: классический резолв реальных IP-адресов через апстрим DNS-сервера.<br />
                    <b>Когда использовать</b>: если в вашей локальной сети критически необходим доступ к доменам роутера (<K>.keenetic.io</K>, <K>.keenetic.pro</K>, DLNA/SMB серверам или локальным именам устройств Home LAN).
                  </>
                ),
              },
            ]}
          />
        </SectionCard>
      )}

      {/* 13. ОПТИМИЗАЦИЯ ОПЕРАТИВНОЙ ПАМЯТИ */}
      {shouldShow('ram', 'память ram оптимизация озу gomemlimit gogc memconservative xkeen-ui 1000 утечки производительность') && (
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

      {/* 14. ЛОГИ И SYSLOG */}
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

      {/* 15. REST API */}
      {shouldShow('api', 'api rest curl скрипты статус серверы эндпоинты json websocket speedtest') && (
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
                name: 'POST /api/failover/check & POST /api/failover/toggle',
                desc: 'Принудительный запуск цикла проверки отказоустойчивости и включение/выключение службы failover.',
              },
              {
                name: 'GET /api/antigravity/status & POST /api/antigravity/settings',
                desc: 'Статус службы ускорения Google AI, порты локального прокси и параметры авто-подстановки DNS.',
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
