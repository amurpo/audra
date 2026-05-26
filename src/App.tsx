import { AudraLanding } from "./components/AudraLanding";
import { I18nProvider } from "./i18n";

export default function App() {
  return (
    <I18nProvider>
      <AudraLanding />
    </I18nProvider>
  );
}
