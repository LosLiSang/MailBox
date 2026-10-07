import { useEffect, useState } from "react";
import { api, type Account, type CacheStats, type Settings, type SettingsView } from "../api";
import { formatSize, move, parseSenderList } from "../format";
import { providerById } from "../providers";
import { Avatar } from "./Avatar";

type Tab = "accounts" | "sync" | "reading" | "proxy" | "appearance" | "advanced";

const TABS: { id: Tab; label: string }[] = [
  { id: "accounts", label: "账号" },
  { id: "sync", label: "同步与缓存" },
  { id: "reading", label: "阅读与隐私" },
  { id: "proxy", label: "代理" },
  { id: "appearance", label: "外观" },
  { id: "advanced", label: "高级" },
];

type Props = {
  settings: SettingsView;
  accounts: Account[];
  initialTab?: Tab;
  onClose: () => void;
  onSettingsSaved: (s: SettingsView) => void;
  onAccountsChanged: (accounts: Account[]) => void;
  onAddAccount: () => void;
  onEditAccount: (account: Account) => void;
  onRemoveAccount: (account: Account) => void;
  onCacheCleared: () => void;
  notify: (text: string) => void;
};

export type { Tab as SettingsTab };

export function SettingsPage(props: Props) {
  const { settings, initialTab = "accounts", onClose } = props;
  const [tab, setTab] = useState<Tab>(initialTab);
  // 去掉 hasProxyPassword，否则和 stripView(settings) 比较时永远不相等
  const [draft, setDraft] = useState<Settings>(() => stripView(settings));
  /** undefined: 不修改；"": 清除 */
  const [proxyPassword, setProxyPassword] = useState<string | undefined>(undefined);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setTab(initialTab), [initialTab]);

  const dirty = JSON.stringify(stripView(settings)) !== JSON.stringify(draft) || proxyPassword !== undefined;

  async function save() {
    setSaving(true);
    setError(null);
    try {
      const saved = await api.saveSettings(draft, proxyPassword);
      props.onSettingsSaved(saved);
      setDraft(stripView(saved));
      setProxyPassword(undefined);
      props.notify("设置已保存");
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  function discard() {
    setDraft(stripView(settings));
    setProxyPassword(undefined);
    setError(null);
  }

  function close() {
    if (dirty && !confirm("有未保存的修改，确定放弃吗？")) return;
    onClose();
  }

  return (
    <div className="settings">
      <nav className="settings-nav">
        <button className="ghost settings-back" onClick={close}>
          ‹ 返回
        </button>
        <h1>设置</h1>
        {TABS.map((t) => (
          <button key={t.id} className={`settings-tab ${tab === t.id ? "active" : ""}`} onClick={() => setTab(t.id)}>
            {t.label}
          </button>
        ))}
      </nav>

      <div className="settings-main">
        <div className="settings-content">
          {tab === "accounts" && <AccountsTab {...props} />}
          {tab === "sync" && <SyncTab draft={draft} setDraft={setDraft} {...props} />}
          {tab === "reading" && <ReadingTab draft={draft} setDraft={setDraft} />}
          {tab === "proxy" && (
            <ProxyTab
              draft={draft}
              setDraft={setDraft}
              hasPassword={settings.hasProxyPassword}
              password={proxyPassword}
              setPassword={setProxyPassword}
            />
          )}
          {tab === "appearance" && <AppearanceTab draft={draft} setDraft={setDraft} />}
          {tab === "advanced" && <AdvancedTab draft={draft} setDraft={setDraft} />}
        </div>

        {tab !== "accounts" && (
          <footer className="settings-footer">
            {error && <span className="error">{error}</span>}
            <button className="ghost" onClick={discard} disabled={!dirty || saving}>
              撤销修改
            </button>
            <button className="primary" onClick={save} disabled={!dirty || saving}>
              {saving ? "保存中…" : "保存"}
            </button>
          </footer>
        )}
      </div>
    </div>
  );
}

function stripView({ hasProxyPassword: _, ...s }: SettingsView | Settings & { hasProxyPassword?: boolean }): Settings {
  return s;
}

type DraftProps = { draft: Settings; setDraft: (s: Settings) => void };

function Section({ title, desc, children }: { title: string; desc?: string; children: React.ReactNode }) {
  return (
    <section className="settings-section">
      <h2>{title}</h2>
      {desc && <p className="muted">{desc}</p>}
      {children}
    </section>
  );
}

// ---------- 账号 ----------

function AccountsTab({ accounts, onAccountsChanged, onAddAccount, onEditAccount, onRemoveAccount, notify }: Props) {
  const [testing, setTesting] = useState<string | null>(null);

  async function reorder(from: number, to: number) {
    const next = move(accounts, from, to);
    if (next === accounts) return;
    onAccountsChanged(next);
    try {
      onAccountsChanged(await api.reorderAccounts(next.map((a) => a.email)));
    } catch (err) {
      notify(`调整顺序失败: ${err}`);
    }
  }

  async function test(email: string) {
    setTesting(email);
    try {
      await api.testAccount(email);
      notify(`✅ ${email} 连接正常`);
    } catch (err) {
      notify(`❌ ${email}: ${err}`);
    } finally {
      setTesting(null);
    }
  }

  return (
    <Section title="邮箱账号" desc="授权码和登录凭据保存在系统凭据管理器中，不会写入任何文件。">
      <ul className="account-rows">
        {accounts.map((a, i) => {
          const p = providerById(a.provider);
          return (
            <li key={a.email}>
              <Avatar name={a.displayName || a.email} seed={a.email} size={32} />
              <div className="account-row-text">
                <div>
                  {a.displayName || a.email.split("@")[0]}
                  <span className="tag">{p.icon} {p.name}</span>
                  <span className="tag">{a.auth === "password" ? p.secretLabel : "OAuth"}</span>
                  {a.useProxy && <span className="tag">代理</span>}
                </div>
                <div className="muted">
                  {a.email} · {a.host}:{a.port}
                </div>
              </div>
              <div className="row-actions">
                <button className="ghost small" onClick={() => reorder(i, i - 1)} disabled={i === 0} title="上移">
                  ↑
                </button>
                <button
                  className="ghost small"
                  onClick={() => reorder(i, i + 1)}
                  disabled={i === accounts.length - 1}
                  title="下移"
                >
                  ↓
                </button>
                <button className="ghost small" onClick={() => test(a.email)} disabled={testing !== null}>
                  {testing === a.email ? "测试中…" : "测试连接"}
                </button>
                <button className="ghost small" onClick={() => onEditAccount(a)}>
                  编辑
                </button>
                <button className="ghost small danger" onClick={() => onRemoveAccount(a)}>
                  删除
                </button>
              </div>
            </li>
          );
        })}
      </ul>
      <button className="primary" onClick={onAddAccount}>
        ＋ 添加账号
      </button>
    </Section>
  );
}

// ---------- 同步与缓存 ----------

function SyncTab({ draft, setDraft, onCacheCleared, notify }: DraftProps & Props) {
  const [stats, setStats] = useState<CacheStats | null>(null);
  const [clearing, setClearing] = useState(false);

  const refresh = () => api.cacheStats().then(setStats).catch((e) => notify(String(e)));
  useEffect(() => {
    refresh();
  }, []);

  async function clear(bodiesOnly: boolean) {
    const msg = bodiesOnly
      ? "清除已下载的邮件正文和附件？邮件列表会保留，再次打开邮件时重新下载。"
      : "清空全部本地缓存？下次打开会从服务器重新同步。";
    if (!confirm(msg)) return;
    setClearing(true);
    try {
      await api.clearCache(bodiesOnly);
      await refresh();
      if (!bodiesOnly) onCacheCleared();
      notify("缓存已清除");
    } catch (err) {
      notify(`清除失败: ${err}`);
    } finally {
      setClearing(false);
    }
  }

  const sync = draft.sync;
  return (
    <>
      <Section title="同步">
        <label className="field">
          <span>每个文件夹最多保留</span>
          <select
            value={sync.window}
            onChange={(e) => setDraft({ ...draft, sync: { ...sync, window: Number(e.currentTarget.value) } })}
          >
            {[100, 200, 500, 1000, 2000, 5000].map((n) => (
              <option key={n} value={n}>
                最近 {n} 封
              </option>
            ))}
          </select>
        </label>
        <p className="muted">首次同步拉取的邮件数量；数量越多首次同步越慢。</p>
        <label className="field">
          <span>自动同步</span>
          <select
            value={sync.autoSyncMinutes}
            onChange={(e) => setDraft({ ...draft, sync: { ...sync, autoSyncMinutes: Number(e.currentTarget.value) } })}
          >
            <option value={0}>关闭</option>
            {[1, 5, 10, 15, 30, 60].map((n) => (
              <option key={n} value={n}>
                每 {n} 分钟
              </option>
            ))}
          </select>
        </label>
      </Section>

      <Section title="本地缓存" desc="邮件头和打开过的正文保存在本机，离线也能查看。">
        {stats ? (
          <>
            <table className="cache-table">
              <thead>
                <tr>
                  <th>账号</th>
                  <th>邮件</th>
                  <th>已下载正文</th>
                </tr>
              </thead>
              <tbody>
                {stats.accounts.map((a) => (
                  <tr key={a.email}>
                    <td>{a.email}</td>
                    <td>{a.headers} 封</td>
                    <td>
                      {a.bodies} 封 · {formatSize(a.bodyBytes)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="muted">数据库文件共 {formatSize(stats.fileBytes)}</p>
          </>
        ) : (
          <p className="muted">读取中…</p>
        )}
        <div className="button-row">
          <button className="ghost" onClick={() => clear(true)} disabled={clearing}>
            清除已下载正文
          </button>
          <button className="ghost danger" onClick={() => clear(false)} disabled={clearing}>
            清空全部缓存
          </button>
        </div>
      </Section>
    </>
  );
}

// ---------- 阅读与隐私 ----------

function ReadingTab({ draft, setDraft }: DraftProps) {
  const reading = draft.reading;
  const [text, setText] = useState(reading.trustedSenders.join("\n"));

  useEffect(() => setText(reading.trustedSenders.join("\n")), [reading.trustedSenders]);

  return (
    <>
      <Section title="远程图片" desc="很多营销邮件用远程图片（追踪像素）记录你是否、何时、在哪打开了邮件。">
        <label className="radio">
          <input
            type="radio"
            checked={reading.remoteImages === "block"}
            onChange={() => setDraft({ ...draft, reading: { ...reading, remoteImages: "block" } })}
          />
          默认拦截，点击后显示（推荐）
        </label>
        <label className="radio">
          <input
            type="radio"
            checked={reading.remoteImages === "allow"}
            onChange={() => setDraft({ ...draft, reading: { ...reading, remoteImages: "allow" } })}
          />
          总是显示
        </label>
      </Section>

      <Section title="信任的发件人" desc="拦截模式下，这些发件人的图片会自动显示。每行一个邮箱地址，或用 @域名 信任整个域名。">
        <textarea
          rows={6}
          placeholder={"noreply@github.com\n@company.com"}
          value={text}
          onChange={(e) => setText(e.currentTarget.value)}
          onBlur={() => setDraft({ ...draft, reading: { ...reading, trustedSenders: parseSenderList(text) } })}
        />
      </Section>

      <Section
        title="深色模式下的邮件正文"
        desc="仅在应用使用深色主题时生效。阅读区右上角可以随时临时切换。"
      >
        <label className="radio">
          <input
            type="radio"
            checked={reading.emailDarkMode === "auto"}
            onChange={() => setDraft({ ...draft, reading: { ...reading, emailDarkMode: "auto" } })}
          />
          智能（推荐）
        </label>
        <p className="muted radio-desc">
          普通来信用深色；邮件自带深色适配时交给邮件自己；有背景色、表格排版的营销邮件和账单保持原样，避免配色错乱
        </p>
        <label className="radio">
          <input
            type="radio"
            checked={reading.emailDarkMode === "always"}
            onChange={() => setDraft({ ...draft, reading: { ...reading, emailDarkMode: "always" } })}
          />
          总是深色
        </label>
        <p className="muted radio-desc">排版复杂的邮件也反色显示（图片保持原色），偶尔会有配色不自然的地方</p>
        <label className="radio">
          <input
            type="radio"
            checked={reading.emailDarkMode === "never"}
            onChange={() => setDraft({ ...draft, reading: { ...reading, emailDarkMode: "never" } })}
          />
          从不
        </label>
        <p className="muted radio-desc">邮件正文始终白底，和发件人看到的一致</p>
      </Section>

      <Section title="已读">
        <label className="checkbox">
          <input
            type="checkbox"
            checked={reading.markReadOnOpen}
            onChange={(e) => setDraft({ ...draft, reading: { ...reading, markReadOnOpen: e.currentTarget.checked } })}
          />
          打开邮件时自动标为已读
        </label>
      </Section>
    </>
  );
}

// ---------- 代理 ----------

type ProxyProps = DraftProps & {
  hasPassword: boolean;
  password: string | undefined;
  setPassword: (p: string | undefined) => void;
};

function ProxyTab({ draft, setDraft, hasPassword, password, setPassword }: ProxyProps) {
  const proxy = draft.proxy;
  const [target, setTarget] = useState("imap.gmail.com:993");
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [testing, setTesting] = useState(false);
  const set = (p: Partial<Settings["proxy"]>) => setDraft({ ...draft, proxy: { ...proxy, ...p } });

  async function test() {
    setTesting(true);
    setResult(null);
    try {
      const ms = await api.testProxy(proxy, target, password);
      setResult({ ok: true, text: `✅ 连接成功，耗时 ${ms} ms` });
    } catch (err) {
      setResult({ ok: false, text: `❌ ${err}` });
    } finally {
      setTesting(false);
    }
  }

  const enabled = proxy.kind !== "none";
  return (
    <>
      <Section
        title="代理服务器"
        desc="用于连接 Gmail 等在国内无法直连的服务器，以及 Google / Microsoft 登录。在账号设置里勾选「通过代理连接」的账号才会使用。"
      >
        <label className="field">
          <span>类型</span>
          <select value={proxy.kind} onChange={(e) => set({ kind: e.currentTarget.value as Settings["proxy"]["kind"] })}>
            <option value="none">不使用代理</option>
            <option value="socks5">SOCKS5</option>
            <option value="http">HTTP</option>
          </select>
        </label>
        {enabled && (
          <>
            <div className="server-row">
              <label className="grow">
                地址
                <input placeholder="127.0.0.1" value={proxy.host} onChange={(e) => set({ host: e.currentTarget.value })} />
              </label>
              <label className="port">
                端口
                <input
                  type="number"
                  placeholder="7890"
                  value={proxy.port || ""}
                  onChange={(e) => set({ port: Number(e.currentTarget.value) })}
                />
              </label>
            </div>
            <div className="server-row">
              <label className="grow">
                用户名 <span className="optional">可选</span>
                <input value={proxy.username} onChange={(e) => set({ username: e.currentTarget.value })} />
              </label>
              <label className="grow">
                密码 <span className="optional">{hasPassword && password === undefined ? "已保存，留空不修改" : "可选"}</span>
                <input
                  type="password"
                  value={password ?? ""}
                  placeholder={hasPassword && password === undefined ? "••••••••" : ""}
                  onChange={(e) => setPassword(e.currentTarget.value)}
                />
              </label>
            </div>
            {hasPassword && (
              <button className="link" onClick={() => setPassword("")}>
                清除已保存的代理密码
              </button>
            )}
            <p className="muted">Clash / v2rayN 等工具一般使用 127.0.0.1:7890（混合端口）或 127.0.0.1:10808（SOCKS5）。</p>
          </>
        )}
      </Section>

      {enabled && (
        <Section title="测试连接" desc="使用当前填写（尚未保存）的代理，连接下面的服务器并完成 TLS 握手。">
          <div className="server-row">
            <input className="grow" value={target} onChange={(e) => setTarget(e.currentTarget.value)} />
            <button className="ghost" onClick={test} disabled={testing}>
              {testing ? "测试中…" : "测试"}
            </button>
          </div>
          {result && <p className={result.ok ? "success" : "error"}>{result.text}</p>}
        </Section>
      )}
    </>
  );
}

// ---------- 外观 ----------

function AppearanceTab({ draft, setDraft }: DraftProps) {
  const a = draft.appearance;
  return (
    <Section title="外观">
      <label className="field">
        <span>主题</span>
        <select
          value={a.theme}
          onChange={(e) =>
            setDraft({ ...draft, appearance: { ...a, theme: e.currentTarget.value as Settings["appearance"]["theme"] } })
          }
        >
          <option value="system">跟随系统</option>
          <option value="light">浅色</option>
          <option value="dark">深色</option>
        </select>
      </label>
      <label className="field">
        <span>列表密度</span>
        <select
          value={a.density}
          onChange={(e) =>
            setDraft({
              ...draft,
              appearance: { ...a, density: e.currentTarget.value as Settings["appearance"]["density"] },
            })
          }
        >
          <option value="comfortable">舒适</option>
          <option value="compact">紧凑</option>
        </select>
      </label>
    </Section>
  );
}

// ---------- 高级：OAuth 应用 ----------

function AdvancedTab({ draft, setDraft }: DraftProps) {
  const o = draft.oauth;
  const set = (p: Partial<Settings["oauth"]>) => setDraft({ ...draft, oauth: { ...o, ...p } });
  return (
    <>
      <Section
        title="Google OAuth 应用"
        desc="Gmail 浏览器登录需要你自己在 Google Cloud 创建一个「桌面应用」类型的 OAuth 客户端。步骤见项目里的 docs/oauth-setup.md。"
      >
        <label>
          Client ID
          <input
            placeholder="xxxx.apps.googleusercontent.com"
            value={o.googleClientId}
            onChange={(e) => set({ googleClientId: e.currentTarget.value })}
          />
        </label>
        <label>
          Client Secret
          <input
            type="password"
            placeholder="GOCSPX-..."
            value={o.googleClientSecret}
            onChange={(e) => set({ googleClientSecret: e.currentTarget.value })}
          />
        </label>
        <button className="link" onClick={() => api.openExternal("https://console.cloud.google.com/apis/credentials")}>
          打开 Google Cloud Console
        </button>
      </Section>

      <Section
        title="Microsoft OAuth 应用"
        desc="Outlook / Hotmail 登录需要在 Azure 注册一个应用，平台选「移动和桌面应用程序」，重定向 URI 填 http://localhost。"
      >
        <label>
          Application (client) ID
          <input
            placeholder="00000000-0000-0000-0000-000000000000"
            value={o.microsoftClientId}
            onChange={(e) => set({ microsoftClientId: e.currentTarget.value })}
          />
        </label>
        <button
          className="link"
          onClick={() =>
            api.openExternal("https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade")
          }
        >
          打开 Azure 应用注册
        </button>
      </Section>
    </>
  );
}
