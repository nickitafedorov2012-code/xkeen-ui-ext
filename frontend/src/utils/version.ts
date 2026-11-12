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
    if (pA.prerelease > pB.prerelease) return 1
    if (pA.prerelease < pB.prerelease) return -1
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
  const pCurrent = parseVersion(current)
  const pLatest = parseVersion(latest)
  if (!pCurrent || !pLatest) return false

  // First compare base numeric parts
  const maxLen = Math.max(pCurrent.numbers.length, pLatest.numbers.length)
  for (let i = 0; i < maxLen; i++) {
    const numA = pCurrent.numbers[i] ?? 0
    const numB = pLatest.numbers[i] ?? 0
    if (numA > numB) return false // current numeric > latest numeric
    if (numA < numB) return true  // current numeric < latest numeric
  }

  // Numeric parts are equal!
  // E.g. current = 1.17.0-alpha, latest = 1.16.5 was already handled (1.17 > 1.16 => false).
  // What if current = 1.19.0-rc1 and latest = 1.19.0?
  // Or current = 1.16.5 and latest = 1.16.5?
  // Per requirement: "если current >= latest (в т. ч. альфа/rc с большим номером) — обновления нет."
  // When numeric base is identical (1.19.0 vs 1.19.0):
  // If current already has the same base version as latest (even if current is rc/alpha of that version),
  // or current is already equal to latest, there is NO new version release to upgrade to.
  return false
}
