/** Bytes for people: "4.2 GB", "612 KB", "38 B". */
export function formatSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
  return `${bytes} B`;
}
