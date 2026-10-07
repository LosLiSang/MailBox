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

export const api = {
  listAccounts: () => invoke<Account[]>("list_accounts"),
  addAccount: (account: NewAccount) => invoke<Account>("add_account", { account }),
  removeAccount: (email: string) => invoke<void>("remove_account", { email }),
  fetchRecent: (email: string, limit = 50) =>
    invoke<Envelope[]>("fetch_recent", { email, limit }),
};
