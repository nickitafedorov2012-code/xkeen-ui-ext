import React, { useEffect, useState, useRef } from 'react'
import { apiGet, apiPost } from '../api'
import type { ConfigFile } from '../types'
import { copyToClipboard } from '../utils/clipboard'

interface ConfigEditorProps {
  isOpen?: boolean
  onClose: () => void
  notify: (msg: string, error?: boolean) => void
}

function IconFileCode() {
  return (
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z" />
      <polyline points="14 2 14 8 20 8" />
      <path d="m10 13-2 2 2 2" />
      <path d="m14 17 2-2-2-2" />
    </svg>
  )
}

function IconSearch() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  )
}

function IconCopy() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  )
}

export default function ConfigEditor({ isOpen = true, onClose, notify }: ConfigEditorProps) {
  if (!isOpen) return null
  const [files, setFiles] = useState<ConfigFile[]>([])
  const [selectedFile, setSelectedFile] = useState<string>('mihomo')
  const [filePath, setFilePath] = useState<string>('')
  const [content, setContent] = useState<string>('')
  const [originalContent, setOriginalContent] = useState<string>('')
  const [loading, setLoading] = useState<boolean>(true)
  const [saving, setSaving] = useState<boolean>(false)
  const [reloadMihomo, setReloadMihomo] = useState<boolean>(true)
  const [syntaxError, setSyntaxError] = useState<string | null>(null)
  const [searchTerm, setSearchTerm] = useState<string>('')
  const [showSearch, setShowSearch] = useState<boolean>(false)

  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const lineNumbersRef = useRef<HTMLDivElement>(null)

  // Загрузка списка файлов
  useEffect(() => {
    const fetchList = async () => {
      try {
        const list = await apiGet<ConfigFile[]>('config-files/list')
        setFiles(list)
      } catch (err: any) {
        notify('Ошибка загрузки списка файлов: ' + err.message, true)
      }
    }
    fetchList()
  }, [notify])

  // Загрузка содержимого выбранного файла
  useEffect(() => {
    const loadFile = async () => {
      setLoading(true)
      setSyntaxError(null)
      try {
        const res = await apiGet<{ file: string; path: string; content: string }>(
          `config-files/read?file=${encodeURIComponent(selectedFile)}`
        )
        setContent(res.content)
        setOriginalContent(res.content)
        setFilePath(res.path)
      } catch (err: any) {
        notify('Ошибка чтения файла: ' + err.message, true)
      } finally {
        setLoading(false)
      }
    }
    loadFile()
  }, [selectedFile, notify])

  // Проверка синтаксиса YAML/JSON
  useEffect(() => {
    if (!content) {
      setSyntaxError(null)
      return
    }

    const curFile = files.find((f) => f.id === selectedFile)
    if (curFile?.syntax === 'json') {
      try {
        JSON.parse(content)
        setSyntaxError(null)
      } catch (e: any) {
        setSyntaxError('Ошибка JSON: ' + e.message)
      }
    } else if (curFile?.syntax === 'yaml') {
      // Базовая эвристическая проверка YAML (табы вместо пробелов, двоеточия)
      if (content.includes('\t')) {
        setSyntaxError('Внимание: YAML содержит символы табуляции (\\t), замените их на пробелы')
      } else {
        setSyntaxError(null)
      }
    }
  }, [content, selectedFile, files])

  // Синхронизация скролла номеров строк и текстового поля
  const handleScroll = () => {
    if (textareaRef.current && lineNumbersRef.current) {
      lineNumbersRef.current.scrollTop = textareaRef.current.scrollTop
    }
  }

  // Обработка клавиш: Tab, Ctrl+S, Ctrl+F
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Ctrl+S: Сохранить
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
      e.preventDefault()
      handleSave()
      return
    }

    // Ctrl+F: Поиск
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
      e.preventDefault()
      setShowSearch((prev) => !prev)
      return
    }

    // Tab: Вставка 2 пробелов
    if (e.key === 'Tab') {
      e.preventDefault()
      const textarea = textareaRef.current
      if (!textarea) return

      const start = textarea.selectionStart
      const end = textarea.selectionEnd

      if (e.shiftKey) {
        // Shift+Tab: Уменьшить отступ
        const lines = content.substring(0, start).split('\n')
        const currentLineIdx = lines.length - 1
        const allLines = content.split('\n')
        const currentLine = allLines[currentLineIdx]

        if (currentLine.startsWith('  ')) {
          allLines[currentLineIdx] = currentLine.substring(2)
          const newContent = allLines.join('\n')
          setContent(newContent)
          setTimeout(() => {
            textarea.selectionStart = Math.max(0, start - 2)
            textarea.selectionEnd = Math.max(0, end - 2)
          }, 0)
        }
      } else {
        // Tab: Вставить 2 пробела
        const newContent = content.substring(0, start) + '  ' + content.substring(end)
        setContent(newContent)
        setTimeout(() => {
          textarea.selectionStart = start + 2
          textarea.selectionEnd = start + 2
        }, 0)
      }
    }
  }

  // Сохранение файла
  const handleSave = async () => {
    setSaving(true)
    try {
      const res = await apiPost<{ saved: boolean; warning?: string }>('config-files/save', {
        file: selectedFile,
        content,
        reload_mihomo: reloadMihomo,
      })

      if (res.saved) {
        setOriginalContent(content)
        if (res.warning) {
          notify(res.warning, true)
        } else {
          notify(`Файл успешно сохранён${reloadMihomo ? ' и применён в ядре' : ''}`)
        }
      }
    } catch (err: any) {
      notify('Ошибка сохранения: ' + err.message, true)
    } finally {
      setSaving(false)
    }
  }

  const curFile = files.find((f) => f.id === selectedFile)
  const isDirty = content !== originalContent
  const linesCount = content ? content.split('\n').length : 1
  const lineNumbers = Array.from({ length: linesCount }, (_, i) => i + 1)

  return (
    <div className="modal-backdrop editor-backdrop">
      <div className="modal-card editor-card">
        <div className="editor-header">
          <div className="editor-title-group">
            <div
              style={{
                width: 36,
                height: 36,
                borderRadius: 9,
                background: 'linear-gradient(135deg, rgba(56, 189, 248, 0.2), rgba(37, 99, 235, 0.2))',
                border: '1px solid rgba(56, 189, 248, 0.35)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: '#38bdf8',
                flexShrink: 0,
              }}
            >
              <IconFileCode />
            </div>
            <h3 style={{ margin: 0, fontSize: 18, fontWeight: 700 }}>Редактор конфигураций</h3>
            <select
              className="select editor-select"
              value={selectedFile}
              onChange={(e) => {
                if (isDirty && !window.confirm('У вас есть несохранённые изменения. Переключить файл?')) {
                  return
                }
                setSelectedFile(e.target.value)
              }}
            >
              {files.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.name}
                </option>
              ))}
            </select>
          </div>

          <div className="editor-header-actions">
            {showSearch && (
              <div className="editor-search-box">
                <input
                  type="text"
                  placeholder="Поиск в файле (Ctrl+F)…"
                  className="input-text editor-search-input"
                  value={searchTerm}
                  onChange={(e) => setSearchTerm(e.target.value)}
                  autoFocus
                />
              </div>
            )}

            <button
              className="btn btn-sm btn-secondary"
              title="Поиск в файле (Ctrl+F)"
              onClick={() => setShowSearch(!showSearch)}
            >
              <IconSearch />
            </button>

            <button
              className="btn btn-sm btn-secondary"
              title="Копировать всё в буфер"
              onClick={async () => {
                await copyToClipboard(content)
                notify('Скопировано в буфер обмена')
              }}
            >
              <IconCopy />
            </button>

            <button
              className="btn btn-sm ghost"
              title="Закрыть"
              onClick={onClose}
              style={{ fontSize: 16, width: 32, height: 32, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', borderRadius: 8, padding: 0 }}
            >
              ✕
            </button>
          </div>
        </div>

        <div className="editor-meta-bar">
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, overflow: 'hidden' }}>
            <span className="editor-path" title={filePath}>
              📁 <code>{filePath || 'Загрузка…'}</code>
            </span>
            {curFile?.syntax && (
              <span className="badge" style={{ fontSize: 10, background: 'rgba(56, 189, 248, 0.12)', color: '#38bdf8', borderColor: 'rgba(56, 189, 248, 0.3)' }}>
                {curFile.syntax.toUpperCase()}
              </span>
            )}
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexShrink: 0 }}>
            {isDirty && <span className="editor-badge-dirty">● Не сохранено</span>}
            {syntaxError && <span className="editor-badge-err">{syntaxError}</span>}
            <span className="editor-stats">
              Строк: {linesCount} · Символов: {content.length}
            </span>
          </div>
        </div>

        <div className="editor-body">
          {loading ? (
            <div className="editor-loading">⏳ Загрузка содержимого файла…</div>
          ) : (
            <div className="editor-container">
              <div className="editor-line-numbers" ref={lineNumbersRef}>
                <pre style={{ margin: 0, padding: 0, fontFamily: 'inherit', fontSize: 'inherit', lineHeight: 'inherit', color: 'inherit' }}>
                  {lineNumbers.join('\n')}
                </pre>
              </div>

              <textarea
                ref={textareaRef}
                className="editor-textarea"
                value={content}
                onChange={(e) => setContent(e.target.value)}
                onScroll={handleScroll}
                onKeyDown={handleKeyDown}
                spellCheck={false}
                placeholder="Содержимое файла пусто"
              />
            </div>
          )}
        </div>

        <div className="editor-footer">
          <label className="editor-reload-toggle" title="Автоматически отправить PUT /configs в Mihomo после сохранения">
            <input
              type="checkbox"
              checked={reloadMihomo}
              onChange={(e) => setReloadMihomo(e.target.checked)}
            />
            <span>Горячий перезапуск ядра Mihomo (Reload)</span>
          </label>

          <div className="editor-footer-buttons">
            <button className="btn btn-secondary" onClick={onClose}>
              Отмена
            </button>
            <button
              className="btn btn-primary"
              disabled={saving || !isDirty}
              onClick={handleSave}
            >
              {saving ? '⏳ Сохранение…' : '💾 Сохранить файл (Ctrl+S)'}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
