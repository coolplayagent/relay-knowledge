export type ThemeMode = "light" | "dark";

const THEME_STORAGE_KEY = "relay-knowledge-theme";

let activeTheme: ThemeMode = "dark";

export function initializeTheme(): ThemeMode {
  activeTheme = storedTheme() ?? systemTheme();
  applyTheme(activeTheme);

  return activeTheme;
}

export function currentTheme(): ThemeMode {
  return activeTheme;
}

export function toggleTheme(): ThemeMode {
  activeTheme = activeTheme === "dark" ? "light" : "dark";
  applyTheme(activeTheme);
  storeTheme(activeTheme);

  return activeTheme;
}

function applyTheme(theme: ThemeMode) {
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
}

function storedTheme(): ThemeMode | null {
  try {
    const value = window.localStorage.getItem(THEME_STORAGE_KEY);

    return value === "light" || value === "dark" ? value : null;
  } catch {
    return null;
  }
}

function storeTheme(theme: ThemeMode) {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // Storage can be unavailable in restricted browser contexts.
  }
}

function systemTheme(): ThemeMode {
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}
