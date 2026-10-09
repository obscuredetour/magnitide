/** Language names for ISO 639-2 codes ("eng" → "English"), in the user's language. */

const names = (() => {
  try {
    return new Intl.DisplayNames([navigator.language, 'en'], { type: 'language' });
  } catch {
    return null;
  }
})();

export function languageName(code: string): string {
  try {
    return names?.of(code) ?? code;
  } catch {
    return code;
  }
}
