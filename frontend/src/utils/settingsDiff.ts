/**
 * Частичное обновление настроек: только поля, которые пользователь реально изменил
 * относительно загруженного снимка.
 *
 * Бэкенд (PUT /api/settings) мержит тело глубоко поверх текущей конфигурации.
 * Раньше страница настроек отправляла ВЕСЬ снимок с момента открытия — и затирала
 * всё, что за это время поменяли фоновые механизмы (умный игровой режим, failover
 * с цепочками приоритетов, авто-режим БС, сторож Zapret). Конфиг откатывался, а
 * iptables/ядро оставались в новом состоянии — тумблеры и сеть расходились.
 *
 * Объекты сравниваются рекурсивно; массивы и примитивы — целиком по значению.
 * Возвращает undefined, если изменений нет.
 */
export function diffSettings(base: unknown, edited: unknown): unknown {
  const isPlainObject = (v: unknown): v is Record<string, unknown> =>
    typeof v === 'object' && v !== null && !Array.isArray(v)

  if (isPlainObject(base) && isPlainObject(edited)) {
    const out: Record<string, unknown> = {}
    for (const key of Object.keys(edited)) {
      const d = diffSettings(base[key], edited[key])
      if (d !== undefined) out[key] = d
    }
    return Object.keys(out).length > 0 ? out : undefined
  }

  return JSON.stringify(base) === JSON.stringify(edited) ? undefined : edited
}
