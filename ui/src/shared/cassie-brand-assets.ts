const CASSIE_LOGO_FILENAME = "cassie-logo-64x64.png";

export const cassieLogoPath = `/assets/${CASSIE_LOGO_FILENAME}`;
const cassieLogoFallbackPath = `/${CASSIE_LOGO_FILENAME}`;
export const cassieLogoImageProps: Record<string, unknown> = {
  "data-cassie-logo-fallback": "false",
  onError: resetCassieLogoOnFailure,
};

export function resetCassieLogoOnFailure(event: Event) {
  const target = event.currentTarget;
  if (!(target instanceof HTMLImageElement)) {
    return;
  }

  if (target.dataset.cassieLogoFallback === "true") {
    return;
  }

  target.dataset.cassieLogoFallback = "true";
  target.src = cassieLogoFallbackPath;
}
