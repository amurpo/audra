import { useI18n } from "../i18n";
import type { Locale } from "../i18n/types";

const localeOptions: Locale[] = ["en", "es"];

export function LanguageSwitcher() {
  const { locale, copy, setLocale } = useI18n();

  return (
    <div className="flex items-center gap-2">
      <span className="sr-only">{copy.language.label}</span>
      <div
        className="lang-switch"
        role="group"
        aria-label={copy.language.label}
      >
        {localeOptions.map((option) => {
          const active = locale === option;
          return (
            <button
              key={option}
              type="button"
              onClick={() => setLocale(option)}
              aria-pressed={active}
              className="lang-switch-btn"
            >
              {copy.language[option]}
            </button>
          );
        })}
      </div>
    </div>
  );
}
