import { useCallback, useEffect, useRef, useState } from "react";
import { api, type Account, type Category, type EmailDarkMode, type Envelope, type SettingsView } from "./api";
import { AccountDialog } from "./components/AccountDialog";
import { Avatar } from "./components/Avatar";
import { MailList } from "./components/MailList";
import { Reader } from "./components/Reader";
import { SettingsPage, type SettingsTab } from "./components/SettingsPage";
import { syncSummary } from "./format";
import { providerById } from "./providers";
import { useEffectiveDark } from "./theme";
import "./App.css";

type Inbox = {
  /** null 表示本地缓存还没读出来 */
  /** 正在浏览的分类：null 收件箱，-1 全部邮件 */
  view: number | null;
  mails: Envelope[] | null;
  syncing: boolean;
  error: string | null;
};

const EMPTY_INBOX: Inbox = { view: null, mails: null, syncing: false, error: null };

export default function App() {
  const [accounts, setAccounts] = useState<Account[] | null>(null);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [dialog, setDialog] = useState<{ open: boolean; editing: Account | null }>({ open: false, editing: null });
  const [settingsTab, setSettingsTab] = useState<SettingsTab | null>(null);
  const [inboxes, setInboxes] = useState<Record<string, Inbox>>({});
  const [categories, setCategories] = useState<Category[]>([]);
  const [openUid, setOpenUid] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const noticeTimer = useRef<number>(undefined);
  /** 正在同步的账号，避免自动同步和手动同步重叠 */
  const syncing = useRef(new Set<string>());

  const current = accounts?.find((a) => a.email === selected) ?? null;
  const inbox = (selected && inboxes[selected]) || EMPTY_INBOX;
  const openMail = inbox.mails?.find((m) => m.uid === openUid) ?? null;
  const proxyConfigured = Boolean(settings && settings.proxy.kind !== "none" && settings.proxy.host);

  const patch = useCallback((email: string, p: Partial<Inbox>) => {
    setInboxes((all) => ({ ...all, [email]: { ...(all[email] ?? EMPTY_INBOX), ...p } }));
  }, []);

  const showNotice = useCallback((text: string) => {
    setNotice(text);
    window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 4000);
  }, []);

  const loadCached = useCallback(
    async (email: string, view: number | null = null) => {
      try {
        patch(email, { view, mails: await api.listCached(email, view) });
      } catch (err) {
        patch(email, { error: String(err) });
      }
    },
    [patch],
  );

  const sync = useCallback(
    async (email: string, { announce = false } = {}) => {
      if (syncing.current.has(email)) return;
      syncing.current.add(email);
      patch(email, { syncing: true, error: null });
      try {
        const stats = await api.syncInbox(email);
        patch(email, { mails: await api.listCached(email, inboxes[email]?.view ?? null), syncing: false });
        if (announce || stats.added > 0) showNotice(syncSummary(stats));
      } catch (err) {
        // 同步失败时保留缓存，只提示错误（离线也能看）
        patch(email, { syncing: false, error: String(err) });
      } finally {
        syncing.current.delete(email);
      }
    },
    [patch, showNotice, inboxes],
  );

  /** 先秒开本地缓存，再后台增量同步 */
  const load = useCallback(
    async (email: string) => {
      await loadCached(email);
      await sync(email);
    },
    [loadCached, sync],
  );

  useEffect(() => {
    Promise.all([api.listAccounts(), api.getSettings()]).then(([list, s]) => {
      setSettings(s);
      setAccounts(list);
      if (list.length > 0) setSelected(list[0].email);
      else setDialog({ open: true, editing: null });
    });
  }, []);

  const appDark = useEffectiveDark(settings?.appearance.theme ?? "system");
  // 只包含影响正文渲染的设置，变化时重新加载阅读区
  const readingVersion = settings ? JSON.stringify(settings.reading) : "";

  // 外观：主题、密度、字体、字号
  useEffect(() => {
    if (!settings) return;
    const root = document.documentElement;
    root.dataset.theme = settings.appearance.theme;
    root.dataset.density = settings.appearance.density;
    root.style.setProperty("--ui-font", settings.appearance.uiFamily ? `"${settings.appearance.uiFamily}"` : "");
    root.style.setProperty("--ui-scale", String(settings.appearance.uiScale / 100));
    root.style.setProperty("--mail-font", settings.appearance.mailFamily ? `"${settings.appearance.mailFamily}"` : "");
    root.style.setProperty("--mail-size", `${settings.appearance.mailFontSize || 14}px`);
  }, [settings]);

  // 切换账号时加载分类列表
  const refreshCategories = useCallback(
    (email: string) => api.listCategories(email).then(setCategories).catch(() => setCategories([])),
    [],
  );
  useEffect(() => {
    if (selected) refreshCategories(selected);
  }, [selected, refreshCategories]);

  // 切换视图（收件箱 / 分类 / 全部）
  const switchView = useCallback(
    (view: number | null) => {
      if (!selected) return;
      setOpenUid(null);
      loadCached(selected, view);
    },
    [selected, loadCached],
  );

  // 第一次选中某账号时加载
  useEffect(() => {
    if (selected && !inboxes[selected]) load(selected);
  }, [selected, inboxes, load]);

  useEffect(() => setOpenUid(null), [selected]);

  // 自动同步所有账号
  const minutes = settings?.sync.autoSyncMinutes ?? 0;
  useEffect(() => {
    if (!minutes || !accounts?.length) return;
    const timer = window.setInterval(() => {
      for (const a of accounts) {
        // 还没打开过的账号只同步，不需要读缓存
        sync(a.email);
      }
    }, minutes * 60_000);
    return () => window.clearInterval(timer);
  }, [minutes, accounts, sync]);

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
    if (!mail.seen && settings?.reading.markReadOnOpen) setSeen(mail, true);
  }

  /** 移动邮件到分类 / 移回收件箱（view: null） */
  async function moveMail(mail: Envelope, view: number | null) {
    if (!selected) return;
    const email = selected;
    // 乐观更新：先从当前列表里移除
    setInboxes((all) => {
      const box = all[email];
      if (!box?.mails) return all;
      return { ...all, [email]: { ...box, mails: box.mails.filter((m) => m.uid !== mail.uid) } };
    });
    setOpenUid(null);
    try {
      await api.moveMessages(email, [mail.uid], view);
      refreshCategories(email);
    } catch (err) {
      showNotice(`移动失败: ${err}`);
      loadCached(email, inbox.view);
    }
  }

  async function saveReading(reading: Partial<SettingsView["reading"]>, notice: string) {
    if (!settings) return;
    const { hasProxyPassword: _, ...plain } = settings;
    try {
      setSettings(await api.saveSettings({ ...plain, reading: { ...plain.reading, ...reading } }));
      showNotice(notice);
    } catch (err) {
      showNotice(`保存失败: ${err}`);
    }
  }

  function trustSender(entry: string) {
    const trusted = [...new Set([...(settings?.reading.trustedSenders ?? []), entry])];
    return saveReading({ trustedSenders: trusted }, `已信任 ${entry}，可在「设置 → 阅读与隐私」中管理`);
  }

  function setEmailDarkMode(mode: EmailDarkMode) {
    return saveReading(
      { emailDarkMode: mode },
      mode === "always" ? "邮件正文将始终使用深色" : "邮件正文将始终白底原样显示",
    );
  }

  function onSaved(account: Account) {
    const isNew = !accounts?.some((a) => a.email === account.email);
    setAccounts((list) => {
      const rest = list ?? [];
      return rest.some((a) => a.email === account.email)
        ? rest.map((a) => (a.email === account.email ? account : a))
        : [...rest, account];
    });
    setDialog({ open: false, editing: null });
    if (isNew) {
      setInboxes(({ [account.email]: _, ...rest }) => rest);
      setSelected(account.email);
      setSettingsTab(null);
    }
    showNotice(isNew ? `已添加 ${account.email}` : `已保存 ${account.email}`);
  }

  async function onRemove(account: Account) {
    if (
      !confirm(
        `删除账号 ${account.email}？\n\n会删除本地缓存的邮件和保存在系统凭据管理器中的登录凭据，邮箱服务器上的邮件不受影响。`,
      )
    )
      return;
    try {
      await api.removeAccount(account.email);
    } catch (err) {
      return showNotice(`删除失败: ${err}`);
    }
    const rest = (accounts ?? []).filter((a) => a.email !== account.email);
    setAccounts(rest);
    setInboxes(({ [account.email]: _, ...others }) => others);
    if (selected === account.email) setSelected(rest[0]?.email ?? null);
  }

  function onCacheCleared() {
    setInboxes({});
    setOpenUid(null);
  }

  const unread = inbox.mails?.filter((m) => !m.seen).length ?? 0;

  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="brand">📬 MailBox</div>

        <nav className="accounts">
          {accounts?.map((a) => {
            const count = inboxes[a.email]?.mails?.filter((m) => !m.seen).length ?? 0;
            const failed = Boolean(inboxes[a.email]?.error);
            return (
              <button
                key={a.email}
                className={`account ${a.email === selected && !settingsTab ? "active" : ""}`}
                onClick={() => {
                  setSelected(a.email);
                  setSettingsTab(null);
                }}
                title={`${a.email} · ${providerById(a.provider).name}`}
              >
                <Avatar name={a.displayName || a.email} seed={a.email} size={28} />
                <span className="account-text">
                  <span className="account-name">{a.displayName || a.email.split("@")[0]}</span>
                  <span className="account-email">{a.email}</span>
                </span>
                {failed ? (
                  <span className="badge warn" title={inboxes[a.email]?.error ?? ""}>
                    !
                  </span>
                ) : (
                  count > 0 && <span className="badge">{count > 99 ? "99+" : count}</span>
                )}
              </button>
            );
          })}
        </nav>

        {current && (
          <nav className="views">
            <button
              className={`view-item ${inbox.view === null ? "active" : ""}`}
              onClick={() => switchView(null)}
            >
              📥 收件箱
            </button>
            {categories.map((c) => (
              <button
                key={c.id}
                className={`view-item ${inbox.view === c.id ? "active" : ""}`}
                onClick={() => switchView(c.id)}
              >
                <span className="view-dot" style={{ background: c.color }} />
                {c.name}
                <span className="view-count">{c.count}</span>
              </button>
            ))}
            <button className={`view-item ${inbox.view === -1 ? "active" : ""}`} onClick={() => switchView(-1)}>
              🗂 全部邮件
            </button>
          </nav>
        )}

        <div className="sidebar-footer">
          <button className="add-account" onClick={() => setDialog({ open: true, editing: null })}>
            ＋ 添加账号
          </button>
          <button
            className={`sidebar-settings ${settingsTab ? "active" : ""}`}
            onClick={() => setSettingsTab(settingsTab ? null : "accounts")}
          >
            ⚙ 设置
          </button>
        </div>
      </aside>

      {settingsTab && settings && accounts ? (
        <SettingsPage
          settings={settings}
          accounts={accounts}
          initialTab={settingsTab}
          onClose={() => setSettingsTab(null)}
          onSettingsSaved={setSettings}
          onAccountsChanged={setAccounts}
          onAddAccount={() => setDialog({ open: true, editing: null })}
          onEditAccount={(a) => setDialog({ open: true, editing: a })}
          onRemoveAccount={onRemove}
          onCacheCleared={onCacheCleared}
          notify={showNotice}
          email={selected ?? ""}
          categories={categories}
          onCategoriesChanged={(cats) => {
            setCategories(cats);
            if (selected) refreshCategories(selected);
          }}
        />
      ) : current ? (
        <>
          <section className="list-pane">
            <header className="toolbar">
              <div className="toolbar-text">
                <h1>
                  {inbox.view === null
                    ? "收件箱"
                    : inbox.view === -1
                      ? "全部邮件"
                      : categories.find((c) => c.id === inbox.view)?.name ?? "分类"}
                </h1>
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
                  onClick={() => sync(current.email, { announce: true })}
                  disabled={inbox.syncing}
                  title="同步"
                >
                  <span className={inbox.syncing ? "spin" : ""}>⟳</span>
                </button>
                <button
                  className="ghost icon"
                  onClick={() => setDialog({ open: true, editing: current })}
                  title="账号设置"
                >
                  ✎
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
                <MailList
                  mails={inbox.mails}
                  selectedUid={openUid}
                  onSelect={onSelectMail}
                  categories={categories}
                  currentView={inbox.view}
                  onMove={(mail, view) => moveMail(mail, view)}
                />
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
                onTrustSender={trustSender}
                appDark={appDark}
                settingsVersion={readingVersion}
                onDarkModeChange={setEmailDarkMode}
                categories={categories}
                onMove={(view) => openMail && moveMail(openMail, view)}
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
              <button className="primary" onClick={() => setDialog({ open: true, editing: null })}>
                添加账号
              </button>
            </div>
          </main>
        )
      )}

      {notice && <div className="toast">{notice}</div>}

      <AccountDialog
        open={dialog.open}
        editing={dialog.editing}
        proxyConfigured={proxyConfigured}
        onClose={() => setDialog({ open: false, editing: null })}
        onSaved={onSaved}
        onOpenSettings={() => {
          setDialog({ open: false, editing: null });
          setSettingsTab("proxy");
        }}
      />
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
