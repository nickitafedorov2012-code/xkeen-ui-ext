export interface CountryInfo {
  code: string
  flag: string
  name: string
}

/**
 * Преобразует двухбуквенный ISO-код страны в флаг-эмодзи Unicode (Regional Indicator Symbols).
 */
export function countryCodeToFlag(code: string): string {
  if (!code || code.length !== 2) return ''
  const upper = code.toUpperCase()
  const c1 = upper.charCodeAt(0)
  const c2 = upper.charCodeAt(1)
  if (c1 < 65 || c1 > 90 || c2 < 65 || c2 > 90) return ''
  return String.fromCodePoint(127397 + c1, 127397 + c2)
}

/**
 * Преобразует флаг-эмодзи Unicode обратно в двухбуквенный код ISO.
 */
export function flagToCountryCode(flag: string): string | null {
  if (!flag) return null
  const codePoints = [...flag].map((c) => c.codePointAt(0) || 0)
  if (
    codePoints.length === 2 &&
    codePoints[0] >= 127462 &&
    codePoints[0] <= 127487 &&
    codePoints[1] >= 127462 &&
    codePoints[1] <= 127487
  ) {
    const c1 = String.fromCharCode(codePoints[0] - 127397)
    const c2 = String.fromCharCode(codePoints[1] - 127397)
    return `${c1}${c2}`
  }
  return null
}

const COUNTRY_NAMES: Record<string, string> = {
  DE: 'Германия',
  NL: 'Нидерланды',
  GB: 'Великобритания',
  US: 'США',
  FR: 'Франция',
  IT: 'Италия',
  FI: 'Финляндия',
  PL: 'Польша',
  GE: 'Грузия',
  TR: 'Турция',
  KZ: 'Казахстан',
  SE: 'Швеция',
  JP: 'Япония',
  CA: 'Канада',
  ES: 'Испания',
  CH: 'Швейцария',
  AT: 'Австрия',
  SG: 'Сингапур',
  HK: 'Гонконг',
  RU: 'Россия',
  UA: 'Украина',
}

/**
 * Распознает страну по названию сервера, его ID или адресу хоста.
 * Возвращает ISO-код, флаг-эмодзи и локализованное название.
 */
export function detectServerCountry(name: string, id?: string, host?: string): CountryInfo | null {
  const combined = `${name || ''} ${id || ''} ${host || ''}`.trim()
  if (!combined) return null

  // 1. Поиск готового флага-эмодзи в строке
  const flagRegex = /[\uD83C][\uDDE6-\uDDFF][\uD83C][\uDDE6-\uDDFF]/u
  const match = combined.match(flagRegex)
  if (match) {
    const flag = match[0]
    const code = flagToCountryCode(flag)
    if (code) {
      return {
        code,
        flag,
        name: COUNTRY_NAMES[code] || code,
      }
    }
  }

  const lower = combined.toLowerCase()

  // 2. Специфические ключевые слова и теги
  const patterns: [RegExp, string][] = [
    [/(?:^|[-_ \[\(/])gb(?:[-_ \]\)/]|$)|great[ -]?britain|united[ -]?kingdom|london|лондон|великобритан|англия/i, 'GB'],
    [/(?:^|[-_ \[\(/])de(?:[-_ \]\)/]|$)|germany|deutschland|frankfurt|германи|франкфурт/i, 'DE'],
    [/(?:^|[-_ \[\(/])(?:nl|ne)(?:[-_ \]\)/]|$)|netherlands|amsterdam|нидерланд|голланди|амстердам/i, 'NL'],
    [/(?:^|[-_ \[\(/])us(?:[-_ \]\)/]|$)|usa|united[ -]?states|сша|америк|new[ -]?york|chicago|los[ -]?angeles|denver|miami/i, 'US'],
    [/(?:^|[-_ \[\(/])pl(?:[-_ \]\)/]|$)|poland|polska|польш|варшав|warsaw/i, 'PL'],
    [/(?:^|[-_ \[\(/])fi(?:[-_ \]\)/]|$)|finland|suomi|финлянд|хельсинк|helsinki/i, 'FI'],
    [/(?:^|[-_ \[\(/])fr(?:[-_ \]\)/]|$)|france|франци|париж|paris/i, 'FR'],
    [/(?:^|[-_ \[\(/])it(?:[-_ \]\)/]|$)|italy|italia|итали|рим|милан|milan|rome/i, 'IT'],
    [/(?:^|[-_ \[\(/])ge(?:[-_ \]\)/]|$)|georgia|грузи|тбилис|tbilisi/i, 'GE'],
    [/(?:^|[-_ \[\(/])tr(?:[-_ \]\)/]|$)|turkey|türkiye|турци|стамбул|istanbul/i, 'TR'],
    [/(?:^|[-_ \[\(/])kz(?:[-_ \]\)/]|$)|kazakhstan|казахст|алмат|астан|almaty|astana/i, 'KZ'],
    [/(?:^|[-_ \[\(/])se(?:[-_ \]\)/]|$)|sweden|sverige|швеци|стокгольм|stockholm/i, 'SE'],
    [/(?:^|[-_ \[\(/])jp(?:[-_ \]\)/]|$)|japan|япони|токио|tokyo/i, 'JP'],
    [/(?:^|[-_ \[\(/])ca(?:[-_ \]\)/]|$)|canada|канада|торонто|toronto/i, 'CA'],
    [/(?:^|[-_ \[\(/])ru(?:[-_ \]\)/]|$)|russia|росси|москв|питер|moscow|spb/i, 'RU'],
    [/(?:^|[-_ \[\(/])sg(?:[-_ \]\)/]|$)|singapore|сингапур/i, 'SG'],
    [/(?:^|[-_ \[\(/])hk(?:[-_ \]\)/]|$)|hong[ -]?kong|гонконг/i, 'HK'],
    [/(?:^|[-_ \[\(/])es(?:[-_ \]\)/]|$)|spain|испани|мадрид|madrid/i, 'ES'],
    [/(?:^|[-_ \[\(/])ch(?:[-_ \]\)/]|$)|switzerland|швейцари|цюрих|zurich/i, 'CH'],
    [/(?:^|[-_ \[\(/])at(?:[-_ \]\)/]|$)|austria|австри|вена|vienna/i, 'AT'],
  ]

  for (const [re, code] of patterns) {
    if (re.test(lower)) {
      return {
        code,
        flag: countryCodeToFlag(code),
        name: COUNTRY_NAMES[code] || code,
      }
    }
  }

  return null
}
