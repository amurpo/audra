interface AppScreenshotProps {
  src: string;
  alt: string;
}

export function AppScreenshot({ src, alt }: AppScreenshotProps) {
  return (
    <figure>
      <img
        src={src}
        alt={alt}
        className="app-screenshot"
        loading="lazy"
        decoding="async"
      />
    </figure>
  );
}
