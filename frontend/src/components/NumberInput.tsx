import { useEffect, useState } from 'react'

interface Props {
  value: number
  onChange: (val: number) => void
  min?: number
  max?: number
  step?: number
  fallback?: number
  className?: string
  style?: React.CSSProperties
  placeholder?: string
  disabled?: boolean
}

/// Числовое поле ввода, позволяющее полностью очистить инпут для ввода с нуля,
/// не сбрасывая значение моментально на fallback и не мешая набору.
export default function NumberInput({
  value,
  onChange,
  min,
  max,
  step,
  fallback,
  className = 'input',
  style,
  placeholder,
  disabled,
}: Props) {
  const [text, setText] = useState<string>(value !== undefined && value !== null ? String(value) : '')
  const [isFocused, setIsFocused] = useState(false)

  useEffect(() => {
    if (!isFocused) {
      setText(value !== undefined && value !== null ? String(value) : '')
    }
  }, [value, isFocused])

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const raw = e.target.value
    setText(raw)
    if (raw === '') {
      // Позволяет пользователю полностью стереть значение и начать ввод с нуля
      return
    }
    const num = Number(raw)
    if (!isNaN(num)) {
      onChange(num)
    }
  }

  const handleBlur = () => {
    setIsFocused(false)
    if (text === '' || isNaN(Number(text))) {
      const def = fallback !== undefined ? fallback : (min !== undefined ? min : 0)
      setText(String(def))
      onChange(def)
    } else {
      let num = Number(text)
      if (min !== undefined && num < min) num = min
      if (max !== undefined && num > max) num = max
      setText(String(num))
      onChange(num)
    }
  }

  return (
    <input
      type="number"
      className={className}
      style={style}
      min={min}
      max={max}
      step={step}
      placeholder={placeholder}
      disabled={disabled}
      value={text}
      onFocus={() => setIsFocused(true)}
      onChange={handleChange}
      onBlur={handleBlur}
    />
  )
}
