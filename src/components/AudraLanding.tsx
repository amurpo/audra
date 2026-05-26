import { useI18n } from "../i18n";
import heroBg from "../assets/bg.jpg";
import screenshot1 from "../assets/screenshot1.png";
import screenshot2 from "../assets/screenshot2.png";
import { AppScreenshot } from "./AppScreenshot";
import { LanguageSwitcher } from "./LanguageSwitcher";
import { Logo } from "./Logo";

const GITHUB_URL = "https://github.com/amurpo/audra";
const RELEASES_URL = "https://github.com/amurpo/audra/releases";

export function AudraLanding() {
  const { copy } = useI18n();

  return (
    <div className="bg-audra-dark text-audra-white">
      <a href="#main-content" className="skip-link">
        {copy.a11y.skipToContent}
      </a>

      <nav className="site-nav" aria-label="Main navigation">
        <div className="page-container flex items-center justify-between">
          <Logo size={32} />
          <div className="flex flex-wrap items-center justify-end gap-3 sm:gap-4">
            <LanguageSwitcher />
          </div>
        </div>
      </nav>

      <main id="main-content">
        <section
          className="hero-section section-divider section-spacing-hero"
          data-od-id="hero"
          style={{ backgroundImage: `url(${heroBg})` }}
        >
          <div className="hero-overlay" aria-hidden="true" />
          <div className="page-container hero-content">
            <div className="mx-auto max-w-3xl">
              <h1 className="heading-1 mb-6"><Logo size={128} /></h1>
              <p className="body-large mb-8 opacity-85">{copy.hero.subtitle}</p>
              <p className="body-large mb-12 max-w-2xl opacity-75">
                {copy.hero.description}
              </p>
              <div className="flex flex-col gap-4 md:flex-row pt-8">
                <a
                  href={RELEASES_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="btn btn-primary"
                >
                  {copy.hero.downloadNow}
                </a>
                <a
                  href={GITHUB_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="btn btn-secondary"
                >
                  {copy.hero.viewOnGitHub}
                </a>
              </div>
            </div>
          </div>
        </section>

        <section className="section-divider section-spacing" data-od-id="overview">
          <div className="page-container">
            <div className="grid grid-cols-1 items-center gap-12 lg:grid-cols-2">
              <div>
                <h2 className="heading-2 mb-8">{copy.overview.title}</h2>
                <p className="body-text mb-6 opacity-85">{copy.overview.p1}</p>
                <p className="body-text opacity-85">{copy.overview.p2}</p>
              </div>
              <AppScreenshot src={screenshot1} alt={copy.a11y.screenshot1Alt} />
            </div>
          </div>
        </section>

        <section className="section-divider section-spacing" data-od-id="features">
          <div className="page-container">
            <h2 className="heading-2 mb-16">{copy.features.title}</h2>
            <div className="grid grid-cols-1 gap-8 md:grid-cols-2 lg:grid-cols-3">
              {copy.features.items.map((feature) => (
                <div key={feature.title} className="card">
                  <h3 className="heading-3 mb-3">{feature.title}</h3>
                  <p className="body-text opacity-75">{feature.desc}</p>
                </div>
              ))}
            </div>
          </div>
        </section>

        <section className="section-divider section-spacing" data-od-id="why-audra">
          <div className="page-container">
            <div className="max-w-3xl">
              <h2 className="heading-2 mb-8">{copy.whyAudra.title}</h2>
              <div className="space-y-6">
                {copy.whyAudra.items.map((item) => (
                  <div key={item.title}>
                    <h3 className="heading-3 mb-3 text-audra-blue">
                      {item.title}
                    </h3>
                    <p className="body-text opacity-85">{item.desc}</p>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </section>

        <section
          className="section-divider section-spacing"
          data-od-id="how-it-works"
        >
          <div className="page-container">
            <h2 className="heading-2 mb-16">{copy.howItWorks.title}</h2>
            <div className="grid grid-cols-1 gap-12 lg:grid-cols-2 lg:items-start">
              <div className="max-w-3xl space-y-6">
                {copy.howItWorks.steps.map((step) => (
                  <div key={step.num} className="flex gap-6">
                    <div className="step-badge">{step.num}</div>
                    <div className="pt-1">
                      <h3 className="heading-3 mb-2">{step.title}</h3>
                      <p className="body-text opacity-75">{step.desc}</p>
                    </div>
                  </div>
                ))}
              </div>
              <AppScreenshot src={screenshot2} alt={copy.a11y.screenshot2Alt} />
            </div>
          </div>
        </section>

        <section className="section-divider section-spacing" data-od-id="installation">
          <div className="page-container">
            <h2 className="heading-2 mb-12">{copy.installation.title}</h2>
            <div className="grid grid-cols-1 gap-12 lg:grid-cols-2">
              <div>
                <h3 className="heading-3-lg mb-6">
                  {copy.installation.gettingStarted.title}
                </h3>
                <p className="body-text mb-6 opacity-85">
                  {copy.installation.gettingStarted.p1}
                </p>
                <p className="body-text mb-6 opacity-85">
                  {copy.installation.gettingStarted.p2}
                </p>
                <p className="body-text opacity-85">
                  {copy.installation.gettingStarted.p3}
                </p>
              </div>
              <div>
                <h3 className="heading-3-lg mb-6">
                  {copy.installation.buildCommands.title}
                </h3>
                <div className="space-y-4">
                  <div>
                    <p className="body-small mb-2 font-semibold tracking-wider uppercase opacity-60">
                      {copy.installation.buildCommands.fromSource}
                    </p>
                    <pre className="code-block">
                      <code>{copy.installation.code.fromSource}</code>
                    </pre>
                  </div>
                  <div>
                    <p className="body-small mb-2 font-semibold tracking-wider uppercase opacity-60">
                      {copy.installation.buildCommands.debian}
                    </p>
                    <pre className="code-block">
                      <code>{copy.installation.code.debian}</code>
                    </pre>
                  </div>
                  <div>
                    <p className="body-small mb-2 font-semibold tracking-wider uppercase opacity-60">
                      {copy.installation.buildCommands.fedora}
                    </p>
                    <pre className="code-block">
                      <code>{copy.installation.code.fedora}</code>
                    </pre>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </section>
      </main>

      <footer
        className="border-t border-audra-border py-16"
        data-od-id="footer"
      >
        <div className="page-container">
          <div className="max-w-3xl">
            <div className="mb-8">
              <h3 className="heading-3 mb-4">{copy.footer.licensing}</h3>
              <p className="body-text opacity-75">
                {copy.footer.licensingPrefix}
                <span className="font-semibold text-audra-white">
                  GPL-3.0-or-later
                </span>
                {copy.footer.licensingSuffix}
              </p>
            </div>
            <div className="flex flex-col items-start justify-between gap-8 border-t border-audra-border pt-8 md:flex-row md:items-center">
              <p className="body-small opacity-60">{copy.footer.copyright}</p>
              <div className="flex gap-6">
                <a
                  href={GITHUB_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="footer-link body-small"
                >
                  {copy.footer.github}
                </a>
                <a href="#" className="footer-link body-small">
                  {copy.footer.issues}
                </a>
                <a href="#" className="footer-link body-small">
                  {copy.footer.discussions}
                </a>
              </div>
            </div>
          </div>
        </div>
      </footer>
    </div>
  );
}
