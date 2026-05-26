import logoAudra from "../assets/logo-audra.svg";

const WORDMARK_RATIO = 0.5;

export function Logo({ size = 32 }: { size?: number }) {
  return (
    <a href={import.meta.env.BASE_URL} className="site-logo" aria-label="Audra">
      <img
        src={logoAudra}
        alt=""
        className="site-logo-mark"
        width={size}
        height={size}
        style={{ width: size, height: size }}
      />
      <span
        className="site-logo-wordmark"
        style={{ fontSize: size * WORDMARK_RATIO }}
      >
        audra
      </span>
    </a>
  );
}
