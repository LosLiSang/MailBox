import { useCallback, useEffect, useState } from "react";
import { api, type Account, type Envelope } from "./api";
import { Avatar } from "./components/Avatar";
import { AccountDialog } from "./components/AccountDialog";
import { MailList } from "./components/MailList";
import "./App.css";

type MailState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ok"; mails: Envelope[]; fetchedAt: Date };

export default function App() {
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  // 按账号缓存，切换账号时不重新拉取
  const [mailsByAccount, setMailsByAccount] = useState<Record<string, MailState>>({});

  const current = accounts?.find((a) => a.email === selected) ?? null;
  const state: MailState = (selected && mailsByAccount[selected]) || { status: "idle" };

  const refresh = useCallback(async (email: string) => {
    setMailsByAccount((m) => ({ ...m, [email]: { status: "loading" } }));
    try {
      const mails = await api.fetchRecent(email, 50);
      setMailsByAccount((m) => ({ ...m, [email]: { status: "ok", mails, fetchedAt: new Date() } }));
    } catch (err) {
      setMailsByAccount((m) => ({ ...m, [email]: { status: "error", message: String(err) } }));
    }
  }, []);

  useEffect(() => {
    api.listAccounts().then((list) => {
      setAccounts(list);
      if (list.length > 0) setSelected(list[0].email);
      else setDialogOpen(true);
    });
  }, []);

  // 选中账号且还没拉取过时自动拉取
  useEffect(() => {
    if (selected && !mailsByAccount[selected]) refresh(selected);
  }, [selected, mailsByAccount, refresh]);

  function onAdded(account: Account) {
    setAccounts((list) => [...(list ?? []).filter((a) => a.email !== account.email), account]);
    setMailsByAccount(({ [account.email]: _, ...rest }) => rest);
    setSelected(account.email);
    setDialogOpen(false);
  }

  async function onRemove(account: Account) {
    if (!confirm(`移除账号 ${account.email}？\n\n会同时删除保存在系统凭据管理器中的授权码，邮箱服务器上的邮件不受影响。`)) return;
    await api.removeAccount(account.email);
    const rest = (accounts ?? []).filter((a) => a.email !== account.email);
    setAccounts(rest);
    setSelected(rest[0]?.email ?? null);
  }

  const unread = state.status === "ok" ? state.mails.filter((m) => !m.seen).length : 0;

  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="brand">📬 MailBox</div>

        <nav className="accounts">
          {accounts?.map((a) => (
            <button
              key={a.email}
              className={`account ${a.email === selected ? "active" : ""}`}
              onClick={() => setSelected(a.email)}
              title={a.email}
            >
              <Avatar name={a.displayName || a.email} seed={a.email} size={28} />
              <span className="account-text">
                <span className="account-name">{a.displayName || a.email.split("@")[0]}</span>
                <span className="account-email">{a.email}</span>
              </span>
            </button>
          ))}
        </nav>

        <button className="add-account" onClick={() => setDialogOpen(true)}>
          ＋ 添加账号
        </button>
      </aside>

      <main className="content">
        {current ? (
          <>
            <header className="toolbar">
              <div>
                <h1>收件箱</h1>
                <span className="subtitle">
                  {current.email}
                  {state.status === "ok" && ` · ${unread} 封未读 · 最近 ${state.mails.length} 封`}
                </span>
              </div>
              <div className="toolbar-actions">
                <button
                  className="ghost"
                  onClick={() => refresh(current.email)}
                  disabled={state.status === "loading"}
                  title="刷新"
                >
                  <span className={state.status === "loading" ? "spin" : ""}>⟳</span> 刷新
                </button>
                <button className="ghost danger" onClick={() => onRemove(current)} title="移除账号">
                  移除
                </button>
              </div>
            </header>

            <section className="mail-area">
              {state.status === "loading" && <Skeleton />}
              {state.status === "error" && (
                <div className="placeholder error">
                  <p>{state.message}</p>
                  <button className="primary" onClick={() => refresh(current.email)}>
                    重试
                  </button>
                </div>
              )}
              {state.status === "ok" &&
                (state.mails.length > 0 ? (
                  <MailList mails={state.mails} />
                ) : (
                  <div className="placeholder">收件箱是空的</div>
                ))}
            </section>
          </>
        ) : (
          accounts && (
            <div className="placeholder welcome">
              <h2>欢迎使用 MailBox</h2>
              <p>添加一个邮箱账号开始使用</p>
              <button className="primary" onClick={() => setDialogOpen(true)}>
                添加账号
              </button>
            </div>
          )
        )}
      </main>

      <AccountDialog open={dialogOpen} onClose={() => setDialogOpen(false)} onAdded={onAdded} />
    </div>
  );
}

function Skeleton() {
  return (
    <ul className="mail-list skeleton">
      {Array.from({ length: 8 }, (_, i) => (
        <li key={i}>
          <span className="avatar" />
          <div className="mail-body">
            <div className="bar short" />
            <div className="bar" />
          </div>
        </li>
      ))}
    </ul>
  );
}
