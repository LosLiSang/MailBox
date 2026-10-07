import { useEffect, useRef, useState } from "react";
import { api, type Account } from "../api";
import { detectProvider } from "../format";

type Props = {
  open: boolean;
  onClose: () => void;
  onAdded: (account: Account) => void;
};

const EMPTY = { email: "", displayName: "", host: "", port: 993, password: "" };

export function AccountDialog({ open, onClose, onAdded }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const [form, setForm] = useState(EMPTY);
  const [hostTouched, setHostTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const provider = detectProvider(form.email);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      setForm(EMPTY);
      setHostTouched(false);
      setError(null);
      dialog.showModal();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);

  function setEmail(email: string) {
    const p = detectProvider(email);
    setForm((f) => ({
      ...f,
      email,
      ...(p && !hostTouched ? { host: p.host, port: p.port } : {}),
    }));
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onAdded(await api.addAccount(form));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <dialog ref={ref} className="dialog" onClose={onClose} onCancel={() => !busy && onClose()}>
      <form onSubmit={submit}>
        <h2>添加邮箱账号</h2>

        <label>
          邮箱地址
          <input
            autoFocus
            type="email"
            placeholder="123456@qq.com"
            value={form.email}
            onChange={(e) => setEmail(e.currentTarget.value)}
            required
          />
        </label>

        <label>
          显示名称 <span className="optional">可选</span>
          <input
            placeholder={provider?.name ?? "工作邮箱"}
            value={form.displayName}
            onChange={(e) => setForm({ ...form, displayName: e.currentTarget.value })}
          />
        </label>

        <label>
          授权码
          <input
            type="password"
            placeholder="不是登录密码"
            value={form.password}
            onChange={(e) => setForm({ ...form, password: e.currentTarget.value })}
            required
          />
        </label>
        {provider && <p className="hint">💡 {provider.hint}</p>}

        <details className="advanced" open={!provider && form.email.includes("@")}>
          <summary>服务器设置</summary>
          <div className="server-row">
            <label className="grow">
              IMAP 服务器
              <input
                placeholder="imap.example.com"
                value={form.host}
                onChange={(e) => {
                  setHostTouched(true);
                  setForm({ ...form, host: e.currentTarget.value });
                }}
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
        </details>

        {error && <p className="error">{error}</p>}

        <div className="actions">
          <button type="button" className="ghost" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button type="submit" className="primary" disabled={busy}>
            {busy ? "正在验证…" : "验证并添加"}
          </button>
        </div>
      </form>
    </dialog>
  );
}
