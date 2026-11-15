/**
 * Semantic/numeric version parsing and comparison for updates.
 * Correctly compares versions with numerical components, optional 'v' prefix,
 * and handles prerelease identifiers (alpha, beta, rc).
 */

interface ParsedVersion {
  numbers: number[]
  prerelease: string | null
}

export function parseVersion(ver: string): ParsedVersion | null {
  if (!ver || typeof ver !== 'string') return null
  const cleaned = ver.trim().replace(/^[vV]/, '')
  if (!cleaned) return null

  // Split prerelease if any: e.g. "1.17.0-alpha" or "1.19.0-rc1"
  const [mainPart, prePart] = cleaned.split('-', 2)
  const segments = mainPart.split('.')
  if (segments.length === 0 || segments.some((s) => !/^\d+$/.test(s))) {
    return null
  }

  const numbers = segments.map((s) => parseInt(s, 10))
  return {
    numbers,
    prerelease: prePart !== undefined ? prePart.toLowerCase() : null,
  }
}

/**
 * Returns:
 *  1 if a > b
 * -1 if a < b
 *  0 if a === b
 * null if either cannot be parsed
 */
export function compareVersions(a: string, b: string): number | null {
  const pA = parseVersion(a)
  const pB = parseVersion(b)
  if (!pA || !pB) return null

  const maxLen = Math.max(pA.numbers.length, pB.numbers.length)
  for (let i = 0; i < maxLen; i++) {
    const numA = pA.numbers[i] ?? 0
    const numB = pB.numbers[i] ?? 0
    if (numA > numB) return 1
    if (numA < numB) return -1
  }

  // If numeric parts are equal, compare prerelease according to SemVer:
  // 1.0.0 > 1.0.0-alpha (a version without prerelease has higher precedence than with prerelease)
  if (pA.prerelease === null && pB.prerelease !== null) {
    return 1
  }
  if (pA.prerelease !== null && pB.prerelease === null) {
    return -1
  }
  if (pA.prerelease !== null && pB.prerelease !== null) {
    const partsA = pA.prerelease.match(/[a-z]+|\d+/gi) ?? []
    const partsB = pB.prerelease.match(/[a-z]+|\d+/gi) ?? []
    const preLen = Math.max(partsA.length, partsB.length)
    for (let i = 0; i < preLen; i++) {
      const segA = partsA[i]
      const segB = partsB[i]
      if (segA === undefined) return -1
      if (segB === undefined) return 1
      const isNumA = /^\d+$/.test(segA)
      const isNumB = /^\d+$/.test(segB)
      if (isNumA && isNumB) {
        const nA = parseInt(segA, 10)
        const nB = parseInt(segB, 10)
        if (nA > nB) return 1
        if (nA < nB) return -1
      } else if (isNumA !== isNumB) {
        // SemVer: numeric identifiers always have lower precedence than non-numeric
        return isNumA ? -1 : 1
      } else {
        if (segA > segB) return 1
        if (segA < segB) return -1
      }
    }
  }

  return 0
}

/**
 * Returns true if and only if:
 * 1) Both versions are valid/parseable
 * 2) latest is strictly greater than current (numeric/semver compare: compareVersions(current, latest) === -1)
 *
 * If current >= latest (in particular current > latest, e.g. alpha/rc of newer version or local dev build), returns false.
 * Also, if latest has a prerelease (like rc/alpha) while current has the same base numeric version (e.g. current=1.19.0-rc1, latest=1.19.0 release candidate vs release),
 * if latest is not strictly a newer release than current, returns false.
 * Specifically: «если current >= latest (в т. ч. альфа/rc с большим номером) — обновления нет».
 * If either string is invalid/unparseable, returns false.
 */
export function isUpdateAvailable(current: string, latest: string): boolean {
  const cmp = compareVersions(current, latest)
  return cmp === -1
}
