const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function formatSize(bytes: number): string {
  if (!bytes) return "—";
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${UNITS[i]}`;
}

export function formatAge(unixSeconds: number | null, now = Date.now() / 1000): string {
  if (unixSeconds == null) return "—";
  const d = Math.max(0, now - unixSeconds);
  if (d < 3600) return `${Math.max(1, Math.round(d / 60))}m`;
  if (d < 86400) return `${Math.round(d / 3600)}h`;
  if (d < 86400 * 60) return `${Math.round(d / 86400)}d`;
  if (d < 86400 * 365 * 2) return `${Math.round(d / (86400 * 30))}mo`;
  return `${Math.round(d / (86400 * 365))}y`;
}

export function formatCount(n: number | null | undefined): string {
  if (n == null) return "—";
  if (n >= 100_000) return `${Math.round(n / 1000)}k`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

export const FLAG_LABEL: Record<string, string> = {
  tinyForCategory: "Too small for category",
  executableInMedia: "Executable in media",
  deadSwarm: "Dead swarm",
  keywordStuffed: "Keyword-stuffed title",
  spoofedSeeders: "Seeder count looks spoofed",
  noSeedData: "No seed data",
};
