import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Account, type Envelope } from "./api";
import { Avatar } from "./components/Avatar";
import { AccountDialog } from "./components/AccountDialog";
import { MailList } from "./components/MailList";
import { Reader } from "./components/Reader";
import { syncSummary } from "./format";
import "./App.css";

type Inbox = {
  /** null 表示本地缓存还没读出来 */
  mails: Envelope[] | null;
  syncing: boolean;
  error: string | null;
  lastSync: Date | null;
};

const EMPTY_INBOX: Inbox = { mails: null, syncing: false, error: null, lastSync: null };

export default function App() {
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [inboxes, setInboxes] = useState<Record<string, Inbox>>({});
  const [openUid, setOpenUid] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const noticeTimer = useRef<number>(undefined);

  const current = accounts?.find((a) => a.email === selected) ?? null;
  const inbox = (selected && inboxes[selected]) || EMPTY_INBOX;
  const openMail = inbox.mails?.find((m) => m.uid === openUid) ?? null;

  const patch = useCallback((email: string, p: Partial<Inbox>) => {
    setInboxes((all) => ({ ...all, [email]: { ...(all[email] ?? EMPTY_INBOX), ...p } }));
  }, []);

  const showNotice = useCallback((text: string) => {
    setNotice(text);
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 4000);
  }, []);

  /** 先秒开本地缓存，再后台增量同步 */
  const load = useCallback(
    async (email: string, { announce = false } = {}) => {
      try {
        patch(email, { mails: await api.listCached(email) });
      } catch (err) {
        patch(email, { error: String(err) });
      }

      patch(email, { syncing: true, error: null });
      try {
        const stats = await api.syncInbox(email);
        patch(email, { mails: await api.listCached(email), syncing: false, lastSync: new Date() });
        if (announce || stats.added > 0) showNotice(syncSummary(stats));
      } catch (err) {
        // 同步失败时保留缓存，只提示错误（离线也能看）
        patch(email, { syncing: false, error: String(err) });
      }
    },
    [patch, showNotice],
  );

  useEffect(() => {
    api.listAccounts().then((list) => {
      setAccounts(list);
      if (list.length > 0) setSelected(list[0].email);
      else setDialogOpen(true);
    });
  }, []);

  // 第一次选中某账号时加载
  useEffect(() => {
    if (selected && !inboxes[selected]) load(selected);
  }, [selected, inboxes, load]);

  useEffect(() => setOpenUid(null), [selected]);

  function updateSeen(email: string, uid: number, seen: boolean) {
    setInboxes((all) => {
      const box = all[email];
      if (!box?.mails) return all;
      return {
        ...all,
        [email]: { ...box, mails: box.mails.map((m) => (m.uid === uid ? { ...m, seen } : m)) },
      };
    });
  }

  async function setSeen(mail: Envelope, seen: boolean) {
    if (!selected) return;
    const email = selected;
    updateSeen(email, mail.uid, seen);
    try {
      await api.setSeen(email, mail.uid, seen);
    } catch (err) {
      updateSeen(email, mail.uid, !seen);
      showNotice(`同步已读状态失败: ${err}`);
    }
  }

  function onSelectMail(mail: Envelope) {
    setOpenUid(mail.uid);
    if (!mail.seen) setSeen(mail, true);
  }

  function onAdded(account: Account) {
    setAccounts((list) => [...(list ?? []).filter((a) => a.email !== account.email), account]);
    setInboxes(({ [account.email]: _, ...rest }) => rest);
    setSelected(account.email);
    setDialogOpen(false);
  }

  async function onRemove(account: Account) {
    if (
      !confirm(
        `移除账号 ${account.email}？\n\n会删除本地缓存的邮件和保存在系统凭据管理器中的授权码，邮箱服务器上的邮件不受影响。`,
      )
    )
      return;
    await api.removeAccount(account.email);
    const rest = (accounts ?? []).filter((a) => a.email !== account.email);
    setAccounts(rest);
    setInboxes(({ [account.email]: _, ...others }) => others);
    setSelected(rest[0]?.email ?? null);
  }

  const unread = inbox.mails?.filter((m) => !m.seen).length ?? 0;

  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="brand">📬 MailBox</div>

        <nav className="accounts">
          {accounts?.map((a) => {
            const count = inboxes[a.email]?.mails?.filter((m) => !m.seen).length ?? 0;
            return (
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
                {count > 0 && <span className="badge">{count > 99 ? "99+" : count}</span>}
              </button>
            );
          })}
        </nav>

        <button className="add-account" onClick={() => setDialogOpen(true)}>
          ＋ 添加账号
        </button>
      </aside>

      {current ? (
        <>
          <section className="list-pane">
            <header className="toolbar">
              <div className="toolbar-text">
                <h1>收件箱</h1>
                <span className="subtitle">
                  {inbox.syncing
                    ? "正在同步…"
                    : inbox.mails
                      ? `${unread} 封未读 · 共 ${inbox.mails.length} 封`
                      : ""}
                </span>
              </div>
              <div className="toolbar-actions">
                <button
                  className="ghost icon"
                  onClick={() => load(current.email, { announce: true })}
                  disabled={inbox.syncing}
                  title="同步"
                >
                  <span className={inbox.syncing ? "spin" : ""}>⟳</span>
                </button>
                <button className="ghost icon danger" onClick={() => onRemove(current)} title="移除账号">
                  ✕
                </button>
              </div>
            </header>

            {inbox.error && (
              <div className="sync-error" title={inbox.error}>
                ⚠️ 同步失败，显示的是本地缓存：{inbox.error}
              </div>
            )}

            <div className="mail-area">
              {inbox.mails === null ? (
                <Skeleton />
              ) : inbox.mails.length > 0 ? (
                <MailList mails={inbox.mails} selectedUid={openUid} onSelect={onSelectMail} />
              ) : inbox.syncing ? (
                <Skeleton />
              ) : (
                <div className="placeholder">收件箱是空的</div>
              )}
            </div>
          </section>

          <section className="reader-pane">
            {openMail ? (
              <Reader
                key={`${current.email}-${openMail.uid}`}
                email={current.email}
                mail={openMail}
                onToggleSeen={(m) => setSeen(m, !m.seen)}
                onNotice={showNotice}
              />
            ) : (
              <div className="placeholder">选择一封邮件阅读</div>
            )}
          </section>
        </>
      ) : (
        accounts && (
          <main className="welcome-pane">
            <div className="placeholder welcome">
              <h2>欢迎使用 MailBox</h2>
              <p>添加一个邮箱账号开始使用</p>
              <button className="primary" onClick={() => setDialogOpen(true)}>
                添加账号
              </button>
            </div>
          </main>
        )
      )}

      {notice && <div className="toast">{notice}</div>}

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
