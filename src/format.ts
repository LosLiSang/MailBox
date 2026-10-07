export type Provider = {
  name: string;
  host: string;
  port: number;
  /** 获取授权码的说明 */
  hint: string;
};

const QQ: Provider = {
  name: "QQ 邮箱",
  host: "imap.qq.com",
  port: 993,
  hint: "网页版 QQ 邮箱 → 设置 → 账号 → 开启 IMAP/SMTP 服务 → 生成授权码",
};
const NETEASE = (domain: string): Provider => ({
  name: `网易 ${domain}`,
  host: `imap.${domain}`,
  port: 993,
  hint: "网页版邮箱 → 设置 → POP3/SMTP/IMAP → 开启 IMAP 服务 → 新增授权密码",
});

const PROVIDERS: Record<string, Provider> = {
  "qq.com": QQ,
  "foxmail.com": QQ,
  "163.com": NETEASE("163.com"),
  "126.com": NETEASE("126.com"),
  "yeah.net": NETEASE("yeah.net"),
};

export function detectProvider(email: string): Provider | undefined {
  const domain = email.trim().split("@")[1]?.toLowerCase();
  return domain ? PROVIDERS[domain] : undefined;
}

export function formatMailDate(iso: string | null, now = new Date()): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return "昨天";
  if (d.getFullYear() === now.getFullYear()) {
    return `${d.getMonth() + 1}月${d.getDate()}日`;
  }
  return d.toLocaleDateString();
}

/** 阅读区的完整时间，如 2025年4月1日 10:00 */
export function formatFullDate(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[i]}`;
}

export function syncSummary(stats: { added: number; deleted: number; updated: number }): string {
  const parts = [
    stats.added && `${stats.added} 封新邮件`,
    stats.deleted && `${stats.deleted} 封已删除`,
    stats.updated && `${stats.updated} 封状态更新`,
  ].filter(Boolean);
  return parts.length ? parts.join("，") : "已是最新";
}

/** 头像文字：取名字首字符（中文取第一个字，英文取首字母大写） */
export function initials(name: string): string {
  const first = Array.from(name.trim())[0];
  return first ? first.toUpperCase() : "?";
}

/** 根据字符串稳定地生成一个色相，让同一发件人头像颜色固定 */
export function hue(seed: string): number {
  let h = 0;
  for (const ch of seed) h = (h * 31 + ch.codePointAt(0)!) % 360;
  return h;
}
