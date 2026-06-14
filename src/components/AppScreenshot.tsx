import { useRef, type MouseEvent } from "react";

interface AppScreenshotProps {
  src: string;
  alt: string;
  /** Parallax speed factor consumed by useScrollEffects. */
  parallax?: number;
}

export function AppScreenshot({ src, alt, parallax = -0.06 }: AppScreenshotProps) {
  const frameRef = useRef<HTMLDivElement>(null);

  const handleMove = (event: MouseEvent<HTMLElement>) => {
    const frame = frameRef.current;
    if (!frame) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const rect = frame.getBoundingClientRect();
    const px = (event.clientX - rect.left) / rect.width - 0.5;
    const py = (event.clientY - rect.top) / rect.height - 0.5;
    frame.style.transform = `perspective(1000px) rotateX(${(-py * 6).toFixed(2)}deg) rotateY(${(px * 8).toFixed(2)}deg) translateY(-6px)`;
  };

  const handleLeave = () => {
    if (frameRef.current) frameRef.current.style.transform = "";
  };

  return (
    <figure data-reveal>
      <div data-parallax={parallax}>
        <div
          ref={frameRef}
          className="screenshot-frame"
          onMouseMove={handleMove}
          onMouseLeave={handleLeave}
        >
          <img
            src={src}
            alt={alt}
            className="app-screenshot"
            loading="lazy"
            decoding="async"
          />
        </div>
      </div>
    </figure>
  );
}
