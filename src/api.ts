import { invoke } from "@tauri-apps/api/core";
import type { RenderMode } from "./theme";

export type Envelope = {
  uid: number;
  subject: string;
  fromName: string;
  fromAddress: string;
  date: string | null;
  seen: boolean;
};

export type AuthKind = "password" | "google" | "microsoft";
export type OAuthProvider = "google" | "microsoft";

export type Account = {
  email: string;
  displayName: string;
  provider: string;
  host: string;
  port: number;
  auth: AuthKind;
  useProxy: boolean;
};

export type AccountInput = Omit<Account, "auth"> & { password: string };

export type SyncStats = { added: number; deleted: number; updated: number };

export type Attachment = { part: number; name: string; mime: string; size: number };

export type MessageView = {
  subject: string;
  from: string;
  to: string;
  cc: string;
  date: string | null;
  /** 带 CSP 的完整 HTML 文档，放进 sandbox iframe 的 srcdoc */
  html: string;
  hasRemoteContent: boolean;
  remoteAllowed: boolean;
  attachments: Attachment[];
  contentKind: ContentKind;
  renderMode: RenderMode;
};

export type ProxyKind = "none" | "http" | "socks5";

/** 深色主题下邮件正文如何显示 */
export type EmailDarkMode = "auto" | "always" | "never";

export type ContentKind = "plain" | "adaptive" | "colored" | "designed" | "dark";

export type Settings = {
  proxy: { kind: ProxyKind; host: string; port: number; username: string };
  sync: { window: number; autoSyncMinutes: number };
  reading: {
    remoteImages: "block" | "allow";
    trustedSenders: string[];
    markReadOnOpen: boolean;
    emailDarkMode: EmailDarkMode;
  };
  appearance: { theme: "system" | "light" | "dark"; density: "comfortable" | "compact" };
  oauth: { googleClientId: string; googleClientSecret: string; microsoftClientId: string };
};

export type SettingsView = Settings & { hasProxyPassword: boolean };

export type CacheStats = {
  fileBytes: number;
  accounts: { email: string; headers: number; bodies: number; bodyBytes: number }[];
};

export const LIST_LIMIT = 500;

export const api = {
  listAccounts: () => invoke<Account[]>("list_accounts"),
  savePasswordAccount: (account: AccountInput, isNew: boolean) =>
    invoke<Account>("save_password_account", { account, isNew }),
  oauthLogin: (provider: OAuthProvider, opts: { loginHint?: string; displayName?: string; useProxy: boolean }) =>
    invoke<Account>("oauth_login", {
      provider,
      loginHint: opts.loginHint ?? null,
      displayName: opts.displayName ?? "",
      useProxy: opts.useProxy,
    }),
  cancelOAuth: () => invoke<void>("cancel_oauth"),
  testAccount: (email: string) => invoke<void>("test_account", { email }),
  reorderAccounts: (order: string[]) => invoke<Account[]>("reorder_accounts", { order }),
  removeAccount: (email: string) => invoke<void>("remove_account", { email }),

  getSettings: () => invoke<SettingsView>("get_settings"),
  /** proxyPassword: undefined 不修改，"" 清除 */
  saveSettings: (settings: Settings, proxyPassword?: string) =>
    invoke<SettingsView>("save_settings", { settings, proxyPassword: proxyPassword ?? null }),
  testProxy: (proxy: Settings["proxy"], target: string, proxyPassword?: string) =>
    invoke<number>("test_proxy", { proxy, target, proxyPassword: proxyPassword ?? null }),
  cacheStats: () => invoke<CacheStats>("cache_stats"),
  clearCache: (bodiesOnly: boolean) => invoke<void>("clear_cache", { bodiesOnly }),

  listCached: (email: string, limit = LIST_LIMIT) => invoke<Envelope[]>("list_cached", { email, limit }),
  syncInbox: (email: string) => invoke<SyncStats>("sync_inbox", { email }),

  getMessage: (
    email: string,
    uid: number,
    sender: string,
    opts: { allowRemote: boolean; appDark: boolean },
  ) => invoke<MessageView>("get_message", { email, uid, sender, ...opts }),
  setSeen: (email: string, uid: number, seen: boolean) => invoke<void>("set_seen", { email, uid, seen }),
  saveAttachment: (email: string, uid: number, part: number) =>
    invoke<string>("save_attachment", { email, uid, part }),
  openAttachment: (email: string, uid: number, part: number) =>
    invoke<string>("open_attachment", { email, uid, part }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
};
