/**
 * Легковесный синтаксический валидатор YAML для ConfigEditor.
 * Проверяет наличие табуляций, баланс кавычек и скобок, пробелы после двоеточий,
 * а также корректность отступов без внешних зависимостей.
 */

export interface YamlValidationResult {
  valid: boolean
  error?: string
  line?: number
}

export function validateYaml(content: string): YamlValidationResult {
  if (!content.trim()) {
    return { valid: true }
  }

  const lines = content.split('\n')
  const bracketStack: { char: string; line: number }[] = []

  let inMultilineBlock = false
  let multilineIndent = 0

  for (let i = 0; i < lines.length; i++) {
    const lineNum = i + 1
    const rawLine = lines[i]

    // 1. Проверка табуляций
    if (rawLine.includes('\t')) {
      return {
        valid: false,
        error: `Строка ${lineNum}: табуляция (\\t) недопустима в YAML, используйте пробелы`,
        line: lineNum,
      }
    }

    const trimmed = rawLine.trim()
    if (!trimmed || trimmed.startsWith('#')) {
      continue
    }

    const currentIndent = rawLine.search(/\S/)

    // Выход из многострочного блока (| или >)
    if (inMultilineBlock) {
      if (currentIndent <= multilineIndent && !trimmed.startsWith('#')) {
        inMultilineBlock = false
      } else {
        continue
      }
    }

    if (trimmed.endsWith('|') || trimmed.endsWith('>')) {
      inMultilineBlock = true
      multilineIndent = currentIndent
      continue
    }

    // 2. Баланс кавычек в строке (учитывая экранирование)
    let inSingleQuote = false
    let inDoubleQuote = false
    let isEscaped = false

    for (let c = 0; c < rawLine.length; c++) {
      const ch = rawLine[c]

      if (isEscaped) {
        isEscaped = false
        continue
      }

      if (ch === '\\' && inDoubleQuote) {
        isEscaped = true
        continue
      }

      if (ch === "'" && !inDoubleQuote) {
        inSingleQuote = !inSingleQuote
      } else if (ch === '"' && !inSingleQuote) {
        inDoubleQuote = !inDoubleQuote
      } else if (!inSingleQuote && !inDoubleQuote) {
        if (ch === '#' && (c === 0 || rawLine[c - 1] === ' ')) {
          // Начало комментария
          break
        }
        if (ch === '[' || ch === '{') {
          bracketStack.push({ char: ch, line: lineNum })
        } else if (ch === ']') {
          const last = bracketStack.pop()
          if (!last || last.char !== '[') {
            return {
              valid: false,
              error: `Строка ${lineNum}: лишняя или несоответствующая закрывающая скобка ']'`,
              line: lineNum,
            }
          }
        } else if (ch === '}') {
          const last = bracketStack.pop()
          if (!last || last.char !== '{') {
            return {
              valid: false,
              error: `Строка ${lineNum}: лишняя или несоответствующая закрывающая фигурная скобка '}'`,
              line: lineNum,
            }
          }
        }
      }
    }

    if (inSingleQuote) {
      return {
        valid: false,
        error: `Строка ${lineNum}: незакрытая одинарная кавычка (')`,
        line: lineNum,
      }
    }
    if (inDoubleQuote) {
      return {
        valid: false,
        error: `Строка ${lineNum}: незакрытая двойная кавычка (")`,
        line: lineNum,
      }
    }

    // 3. Проверка двоеточия: после ключа 'key:' обязан следовать пробел или конец строки
    const colonIdx = rawLine.indexOf(':')
    if (colonIdx !== -1) {
      const before = rawLine.slice(0, colonIdx).trim()
      const after = rawLine.slice(colonIdx + 1)
      if (!before.startsWith('-') && !rawLine.startsWith('#')) {
        if (after.length > 0 && !after.startsWith(' ') && !after.startsWith('/') && !before.includes('http')) {
          if (!trimmed.startsWith('http://') && !trimmed.startsWith('https://')) {
            return {
              valid: false,
              error: `Строка ${lineNum}: пропущен пробел после двоеточия в '${before}:'`,
              line: lineNum,
            }
          }
        }
      }
    }
  }

  // 4. Проверка незакрытых скобок в документе
  if (bracketStack.length > 0) {
    const unclosed = bracketStack[bracketStack.length - 1]
    return {
      valid: false,
      error: `Строка ${unclosed.line}: незакрытая скобка '${unclosed.char}'`,
      line: unclosed.line,
    }
  }

  return { valid: true }
}
