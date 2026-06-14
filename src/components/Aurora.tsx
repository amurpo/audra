/**
 * Animated "liquid" gradient blobs rendered behind the page content. They drift
 * slowly to give the dark backdrop life and provide colour for the glassmorphic
 * surfaces to refract. Purely decorative.
 */
export function Aurora() {
  return (
    <div className="aurora" aria-hidden="true">
      <span className="aurora-blob aurora-blob-1" />
      <span className="aurora-blob aurora-blob-2" />
      <span className="aurora-blob aurora-blob-3" />
      <div className="aurora-grain" />
    </div>
  );
}
