import { useEffect, useSyncExternalStore } from 'react'

const dirtyMap = new Map<string, boolean>()
const listeners = new Set<() => void>()

function emit() {
  for (const fn of listeners) fn()
}

export function setDirty(id: string, dirty: boolean): void {
  const prev = dirtyMap.get(id) ?? false
  if (dirty) {
    dirtyMap.set(id, true)
  } else {
    dirtyMap.delete(id)
  }
  if (prev !== dirty) {
    emit()
  }
}

export function hasAnyDirty(): boolean {
  return dirtyMap.size > 0
}

export function clearAllDirty(): void {
  if (dirtyMap.size > 0) {
    dirtyMap.clear()
    emit()
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function useHasUnsavedChanges(): boolean {
  return useSyncExternalStore(subscribe, hasAnyDirty, hasAnyDirty)
}

export function useDirtyGuard(id: string, isDirty: boolean): void {
  useEffect(() => {
    setDirty(id, isDirty)
    return () => {
      setDirty(id, false)
    }
  }, [id, isDirty])
}

export function confirmDiscardUnsaved(): boolean {
  if (!hasAnyDirty()) return true
  return window.confirm('Есть несохранённые изменения. Перейти без сохранения?')
}
