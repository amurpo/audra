import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  defaultLocale,
  getCopy,
  isLocale,
} from "./translations";
import type { Locale } from "./types";
import type { SiteCopy } from "./types";

const STORAGE_KEY = "audra-locale";

interface I18nContextValue {
  locale: Locale;
  copy: SiteCopy;
  setLocale: (locale: Locale) => void;
}

const I18nContext = createContext<I18nContextValue | null>(null);

function readInitialLocale(): Locale {
  if (typeof window === "undefined") {
    return defaultLocale;
  }

  const params = new URLSearchParams(window.location.search);
  const queryLocale = params.get("lang");
  if (queryLocale && isLocale(queryLocale)) {
    return queryLocale;
  }

  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored && isLocale(stored)) {
    return stored;
  }

  const browser = navigator.language.slice(0, 2);
  if (isLocale(browser)) {
    return browser;
  }

  return defaultLocale;
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(readInitialLocale);

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    localStorage.setItem(STORAGE_KEY, next);

    const url = new URL(window.location.href);
    url.searchParams.set("lang", next);
    window.history.replaceState({}, "", url);
  }, []);

  const copy = useMemo(() => getCopy(locale), [locale]);

  useEffect(() => {
    document.documentElement.lang = locale;
    document.title = copy.meta.title;

    const description = document.querySelector('meta[name="description"]');
    if (description) {
      description.setAttribute("content", copy.meta.description);
    }
  }, [locale, copy]);

  const value = useMemo(
    () => ({ locale, copy, setLocale }),
    [locale, copy, setLocale],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error("useI18n must be used within I18nProvider");
  }
  return context;
}
