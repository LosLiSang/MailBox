import { useEffect, useRef, useState } from "react";
import { api, type Account } from "../api";
import { detectProvider, PROVIDERS, providerById, unsupportedReason, type Provider } from "../providers";

type Props = {
  open: boolean;
  /** 传入时为编辑模式 */
  editing?: Account | null;
  proxyConfigured: boolean;
  onClose: () => void;
  onSaved: (account: Account) => void;
  onOpenSettings: () => void;
};

type Form = {
  email: string;
  displayName: string;
  host: string;
  port: number;
  password: string;
  useProxy: boolean;
};

const emptyForm = (p: Provider): Form => ({
  email: "",
  displayName: "",
  host: p.host,
  port: p.port,
  password: "",
  useProxy: false,
});

export function AccountDialog({ open, editing, proxyConfigured, onClose, onSaved, onOpenSettings }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const [provider, setProvider] = useState<Provider | null>(null);
  const [form, setForm] = useState<Form>(emptyForm(PROVIDERS[0]));
  /** OAuth 服务商也可以改用应用专用密码 */
  const [usePassword, setUsePassword] = useState(false);
  const [busy, setBusy] = useState<null | "verify" | "oauth">(null);
  const [error, setError] = useState<string | null>(null);

  const isEdit = Boolean(editing);
  const isOAuthAccount = editing ? editing.auth !== "password" : false;
  const oauth = provider?.oauth && !usePassword && (!isEdit || isOAuthAccount);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      setError(null);
      setBusy(null);
      setUsePassword(false);
      if (editing) {
        const p = providerById(editing.provider || detectProvider(editing.email)?.id || "custom");
        setProvider(p);
        setForm({ ...editing, password: "" });
      } else {
        setProvider(null);
      }
      dialog.showModal();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  }, [open, editing]);

  function pick(p: Provider) {
    setProvider(p);
    setUsePassword(false);
    setError(null);
    setForm({ ...emptyForm(p), useProxy: Boolean(p.needsProxy && proxyConfigured) });
  }

  function setEmail(email: string) {
    const detected = detectProvider(email);
    setForm((f) => ({
      ...f,
      email,
      // 「其他 IMAP」下输入已知域名时自动填服务器
      ...(provider?.id === "custom" && detected && !f.host ? { host: detected.host, port: detected.port } : {}),
    }));
  }

  async function submitPassword(e: React.FormEvent) {
    e.preventDefault();
    if (!provider) return;
    const blocked = unsupportedReason(form.email);
    if (blocked) return setError(blocked);
    setBusy("verify");
    setError(null);
    try {
      onSaved(await api.savePasswordAccount({ ...form, provider: provider.id }, !isEdit));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function startOAuth() {
    if (!provider?.oauth) return;
    setBusy("oauth");
    setError(null);
    try {
      onSaved(
        await api.oauthLogin(provider.oauth, {
          loginHint: form.email || editing?.email,
          displayName: form.displayName,
          useProxy: form.useProxy,
        }),
      );
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function saveOAuthEdit(e: React.FormEvent) {
    e.preventDefault();
    if (!editing) return;
    setBusy("verify");
    setError(null);
    try {
      onSaved(await api.savePasswordAccount({ ...form, provider: editing.provider, password: "" }, false));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(null);
    }
  }

  function close() {
    if (busy === "oauth") api.cancelOAuth();
    onClose();
  }

  const proxyRow = (
    <label className="checkbox">
      <input
        type="checkbox"
        checked={form.useProxy}
        disabled={!proxyConfigured}
        onChange={(e) => setForm({ ...form, useProxy: e.currentTarget.checked })}
      />
      通过代理连接
      {!proxyConfigured && (
        <button type="button" className="link" onClick={onOpenSettings}>
          （先去设置代理）
        </button>
      )}
      {provider?.needsProxy && proxyConfigured && !form.useProxy && (
        <span className="muted">· 国内网络通常需要代理</span>
      )}
    </label>
  );

  return (
    <dialog ref={ref} className="dialog" onCancel={(e) => (busy ? e.preventDefault() : close())}>
      {!provider ? (
        <div className="dialog-body">
          <h2>添加邮箱账号</h2>
          <div className="provider-grid">
            {PROVIDERS.map((p) => (
              <button key={p.id} type="button" className="provider" onClick={() => pick(p)}>
                <span className="provider-icon">{p.icon}</span>
                {p.name}
              </button>
            ))}
          </div>
          <div className="actions">
            <button type="button" className="ghost" onClick={close}>
              取消
            </button>
          </div>
        </div>
      ) : (
        <form className="dialog-body" onSubmit={oauth ? (isEdit ? saveOAuthEdit : (e) => e.preventDefault()) : submitPassword}>
          <h2>
            {!isEdit && (
              <button type="button" className="back" onClick={() => setProvider(null)} disabled={Boolean(busy)}>
                ‹
              </button>
            )}
            {provider.icon} {isEdit ? `编辑 ${editing!.email}` : provider.name}
          </h2>

          {oauth ? (
            <>
              {!isEdit && (
                <>
                  <p className="hint">点击下方按钮会在浏览器中打开 {provider.name} 登录页，授权后自动返回。</p>
                  <label>
                    邮箱地址 <span className="optional">可选，用于预填登录页</span>
                    <input
                      type="email"
                      placeholder={`you@${provider.domains[0]}`}
                      value={form.email}
                      onChange={(e) => setForm({ ...form, email: e.currentTarget.value })}
                    />
                  </label>
                </>
              )}
              <label>
                显示名称 <span className="optional">可选</span>
                <input
                  placeholder={provider.name}
                  value={form.displayName}
                  onChange={(e) => setForm({ ...form, displayName: e.currentTarget.value })}
                />
              </label>
              {proxyRow}

              {busy === "oauth" ? (
                <div className="oauth-waiting">
                  <span className="spin">⟳</span> 等待浏览器中完成登录…
                  <button type="button" className="link" onClick={() => api.cancelOAuth()}>
                    取消
                  </button>
                </div>
              ) : (
                <button type="button" className="primary oauth-button" onClick={startOAuth} disabled={Boolean(busy)}>
                  {isEdit ? "重新登录" : `使用 ${provider.name} 账号登录`}
                </button>
              )}

              {!isEdit && provider.secretLabel !== "密码" && (
                <button type="button" className="link" onClick={() => setUsePassword(true)}>
                  改用{provider.secretLabel}
                </button>
              )}
            </>
          ) : (
            <>
              <label>
                邮箱地址
                <input
                  autoFocus={!isEdit}
                  type="email"
                  placeholder={provider.domains[0] ? `you@${provider.domains[0]}` : "you@example.com"}
                  value={form.email}
                  onChange={(e) => setEmail(e.currentTarget.value)}
                  disabled={isEdit}
                  required
                />
              </label>
              <label>
                显示名称 <span className="optional">可选</span>
                <input
                  placeholder={provider.name}
                  value={form.displayName}
                  onChange={(e) => setForm({ ...form, displayName: e.currentTarget.value })}
                />
              </label>
              <label>
                {provider.secretLabel}
                {isEdit && <span className="optional">留空表示不修改</span>}
                <input
                  type="password"
                  placeholder={isEdit ? "••••••••" : provider.secretLabel === "密码" ? "" : "不是登录密码"}
                  value={form.password}
                  onChange={(e) => setForm({ ...form, password: e.currentTarget.value })}
                  required={!isEdit}
                />
              </label>
              {provider.hint && (
                <p className="hint">
                  💡 {provider.hint}
                  {provider.helpUrl && (
                    <>
                      {" "}
                      <button type="button" className="link" onClick={() => api.openExternal(provider.helpUrl!)}>
                        打开
                      </button>
                    </>
                  )}
                </p>
              )}

              <details className="advanced" open={provider.id === "custom"}>
                <summary>服务器设置</summary>
                <div className="server-row">
                  <label className="grow">
                    IMAP 服务器
                    <input
                      placeholder="imap.example.com"
                      value={form.host}
                      onChange={(e) => setForm({ ...form, host: e.currentTarget.value })}
                      required
                    />
                  </label>
                  <label className="port">
                    端口
                    <input
                      type="number"
                      value={form.port}
                      onChange={(e) => setForm({ ...form, port: Number(e.currentTarget.value) })}
                      required
                    />
                  </label>
                </div>
                <p className="hint">仅支持 SSL/TLS 直连（通常是 993 端口）</p>
              </details>
              {proxyRow}
            </>
          )}

          {error && <p className="error">{error}</p>}

          <div className="actions">
            <button type="button" className="ghost" onClick={close} disabled={busy === "verify"}>
              取消
            </button>
            {(!oauth || isEdit) && (
              <button type="submit" className="primary" disabled={Boolean(busy)}>
                {busy === "verify" ? "正在验证…" : isEdit ? "保存" : "验证并添加"}
              </button>
            )}
          </div>
        </form>
      )}
    </dialog>
  );
}
