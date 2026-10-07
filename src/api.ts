import { invoke } from "@tauri-apps/api/core";

export type Envelope = {
  uid: number;
  subject: string;
  fromName: string;
  fromAddress: string;
  date: string | null;
  seen: boolean;
};

export type Account = {
  email: string;
  displayName: string;
  host: string;
  port: number;
};

export type NewAccount = Account & { password: string };

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
  attachments: Attachment[];
};

export const LIST_LIMIT = 500;

export const api = {
  listAccounts: () => invoke<Account[]>("list_accounts"),
  addAccount: (account: NewAccount) => invoke<Account>("add_account", { account }),
  removeAccount: (email: string) => invoke<void>("remove_account", { email }),

  listCached: (email: string, limit = LIST_LIMIT) => invoke<Envelope[]>("list_cached", { email, limit }),
  syncInbox: (email: string) => invoke<SyncStats>("sync_inbox", { email }),

  getMessage: (email: string, uid: number, allowRemote = false) =>
    invoke<MessageView>("get_message", { email, uid, allowRemote }),
  setSeen: (email: string, uid: number, seen: boolean) => invoke<void>("set_seen", { email, uid, seen }),
  saveAttachment: (email: string, uid: number, part: number) =>
    invoke<string>("save_attachment", { email, uid, part }),
  openAttachment: (email: string, uid: number, part: number) =>
    invoke<string>("open_attachment", { email, uid, part }),
};
