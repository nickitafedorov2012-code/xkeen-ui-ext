/**
 * Безопасное копирование текста в буфер обмена.
 * В незащищённом контексте HTTP (например, веб-панель роутера по адресу http://192.168.2.1:1001)
 * navigator.clipboard.writeText может вызывать ошибку безопасности (NotAllowedError/TypeError).
 * Функция автоматически переключается на document.execCommand('copy') через временный скрытый элемент textarea.
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  // 1. Попытка через современный Clipboard API (работает на HTTPS и localhost)
  if (navigator?.clipboard && typeof navigator.clipboard.writeText === 'function') {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      // Игнорируем и переходим к запасному варианту
    }
  }

  // 2. Fallback через document.execCommand('copy') для HTTP
  try {
    const ta = document.createElement('textarea')
    ta.value = text
    ta.setAttribute('readonly', '')
    ta.style.position = 'fixed'
    ta.style.top = '-9999px'
    ta.style.left = '-9999px'
    ta.style.opacity = '0'
    document.body.appendChild(ta)
    ta.focus()
    ta.select()
    const success = document.execCommand('copy')
    document.body.removeChild(ta)
    return success
  } catch {
    return false
  }
}
