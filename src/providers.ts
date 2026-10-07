import type { OAuthProvider } from "./api";

export type Provider = {
  id: string;
  name: string;
  icon: string;
  host: string;
  port: number;
  /** 有值时走浏览器 OAuth 登录 */
  oauth?: OAuthProvider;
  /** 密码框的标签 */
  secretLabel: string;
  /** 获取授权码 / 应用专用密码的步骤 */
  hint?: string;
  helpUrl?: string;
  domains: string[];
  /** 国内一般需要代理才能访问 */
  needsProxy?: boolean;
};

export const PROVIDERS: Provider[] = [
  {
    id: "gmail",
    name: "Gmail",
    icon: "🟥",
    host: "imap.gmail.com",
    port: 993,
    oauth: "google",
    secretLabel: "应用专用密码",
    hint: "也可以不用 OAuth：Google 账号开启两步验证后，在「安全性 → 应用专用密码」生成 16 位密码",
    helpUrl: "https://myaccount.google.com/apppasswords",
    domains: ["gmail.com", "googlemail.com"],
    needsProxy: true,
  },
  {
    id: "outlook",
    name: "Outlook / Hotmail",
    icon: "🟦",
    host: "outlook.office365.com",
    port: 993,
    oauth: "microsoft",
    secretLabel: "密码",
    hint: "微软个人邮箱已不支持密码登录 IMAP，请使用浏览器登录",
    domains: ["outlook.com", "hotmail.com", "live.com", "msn.com", "outlook.cn"],
  },
  {
    id: "qq",
    name: "QQ 邮箱",
    icon: "🐧",
    host: "imap.qq.com",
    port: 993,
    secretLabel: "授权码",
    hint: "网页版 QQ 邮箱 → 设置 → 账号 → 开启 IMAP/SMTP 服务 → 生成授权码",
    helpUrl: "https://wx.mail.qq.com/",
    domains: ["qq.com", "foxmail.com", "vip.qq.com"],
  },
  {
    id: "exmail",
    name: "腾讯企业邮",
    icon: "🏢",
    host: "imap.exmail.qq.com",
    port: 993,
    secretLabel: "客户端专用密码",
    hint: "企业邮 → 设置 → 账户 → 开启安全登录后生成客户端专用密码",
    domains: [],
  },
  {
    id: "icloud",
    name: "iCloud",
    icon: "☁️",
    host: "imap.mail.me.com",
    port: 993,
    secretLabel: "App 专用密码",
    hint: "appleid.apple.com → 登录与安全 → App 专用密码",
    helpUrl: "https://appleid.apple.com/",
    domains: ["icloud.com", "me.com", "mac.com"],
  },
  {
    id: "yahoo",
    name: "Yahoo",
    icon: "🟪",
    host: "imap.mail.yahoo.com",
    port: 993,
    secretLabel: "应用密码",
    hint: "Yahoo 账户安全 → 生成应用密码",
    domains: ["yahoo.com", "ymail.com"],
    needsProxy: true,
  },
  {
    id: "aliyun",
    name: "阿里邮箱",
    icon: "🟧",
    host: "imap.aliyun.com",
    port: 993,
    secretLabel: "密码",
    hint: "网页版 → 设置 → 账户与安全 → 开启 IMAP 服务",
    domains: ["aliyun.com"],
  },
  {
    id: "custom",
    name: "其他 IMAP",
    icon: "✉️",
    host: "",
    port: 993,
    secretLabel: "密码",
    domains: [],
  },
];

export function providerById(id: string): Provider {
  return PROVIDERS.find((p) => p.id === id) ?? PROVIDERS[PROVIDERS.length - 1];
}

export function detectProvider(email: string): Provider | undefined {
  const domain = email.trim().split("@")[1]?.toLowerCase();
  return domain ? PROVIDERS.find((p) => p.domains.includes(domain)) : undefined;
}

/**
 * 网易邮箱要求客户端发送 IMAP ID 命令，当前使用的 imap 库无法支持，先明确提示，
 * 避免用户填了授权码却看到含糊的 "Unsafe Login" 错误
 */
export function unsupportedReason(email: string): string | undefined {
  const domain = email.trim().split("@")[1]?.toLowerCase();
  if (domain && ["163.com", "126.com", "yeah.net", "188.com"].includes(domain)) {
    return "网易邮箱暂不支持（服务器要求 IMAP ID 命令，计划更换 IMAP 库后支持）";
  }
  return undefined;
}
