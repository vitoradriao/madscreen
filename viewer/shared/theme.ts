export type Theme = 'dark' | 'light' | 'contrast';
const key = 'ms.theme';
const valid = (value: unknown): value is Theme => value === 'dark' || value === 'light' || value === 'contrast';

export function currentTheme(): Theme {
  const value = document.documentElement.dataset.theme;
  return valid(value) ? value : 'dark';
}

export function videoTheme(): Theme {
  return currentTheme() === 'light' ? 'dark' : currentTheme();
}

function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
  for (const surface of document.querySelectorAll<HTMLElement>('.stage, .tile')) surface.dataset.theme = videoTheme();
}

export function setTheme(theme: Theme): void {
  // Não anuncia preferência salva se o armazenamento falhar.
  localStorage.setItem(key, theme);
  applyTheme(theme);
}

export function initializeTheme(): void {
  let theme: Theme = 'dark';
  try { const saved = localStorage.getItem(key); if (valid(saved)) theme = saved; } catch { /* Grafite sem armazenamento. */ }
  applyTheme(theme);
}
