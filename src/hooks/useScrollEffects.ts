import { useEffect } from "react";

const prefersReducedMotion = () =>
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * Reveals elements marked with `data-reveal` as they scroll into view, and
 * applies a gentle scroll-driven parallax to elements marked with
 * `data-parallax` (the attribute value is the speed factor). Both effects are
 * disabled when the user prefers reduced motion.
 */
export function useScrollEffects() {
  useEffect(() => {
    const reduced = prefersReducedMotion();

    const revealEls = Array.from(
      document.querySelectorAll<HTMLElement>("[data-reveal]"),
    );

    if (reduced) {
      revealEls.forEach((el) => el.classList.add("is-revealed"));
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-revealed");
            observer.unobserve(entry.target);
          }
        }
      },
      { threshold: 0.15, rootMargin: "0px 0px -10% 0px" },
    );
    revealEls.forEach((el) => observer.observe(el));

    const parallaxEls = Array.from(
      document.querySelectorAll<HTMLElement>("[data-parallax]"),
    );

    let frame = 0;
    const update = () => {
      frame = 0;
      const viewport = window.innerHeight;
      for (const el of parallaxEls) {
        const speed = Number(el.dataset.parallax) || 0;
        const rect = el.getBoundingClientRect();
        const offset = (rect.top + rect.height / 2 - viewport / 2) * speed;
        el.style.setProperty("--parallax-y", `${offset.toFixed(1)}px`);
      }
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };

    update();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll, { passive: true });

    return () => {
      observer.disconnect();
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);
}
