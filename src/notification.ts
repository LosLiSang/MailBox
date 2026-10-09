import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

export type MailSummary = {
  fromName?: string;
  fromAddress?: string;
  subject?: string;
};

/** 格式化新邮件通知标题与内容（纯逻辑） */
export function formatNewMailNotification(
  accountName: string,
  addedCount: number,
  firstMail?: MailSummary,
): { title: string; body: string } {
  if (addedCount <= 0) {
    return { title: "", body: "" };
  }

  if (addedCount === 1 && firstMail) {
    const sender = firstMail.fromName || firstMail.fromAddress || accountName;
    const subject = firstMail.subject?.trim() || "（无主题）";
    return {
      title: `${sender} - 新邮件`,
      body: subject,
    };
  }

  const latestDesc = firstMail?.subject?.trim()
    ? `\n最新：${firstMail.subject.trim()}`
    : "";

  return {
    title: `${accountName} 收到 ${addedCount} 封新邮件`,
    body: latestDesc ? latestDesc.trim() : `点击查看邮件详情`,
  };
}

let permissionChecked = false;
let permissionGranted = false;

async function ensureNotificationPermission(): Promise<boolean> {
  if (permissionChecked) return permissionGranted;
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }
    permissionChecked = true;
    permissionGranted = granted;
    return granted;
  } catch (err) {
    console.warn("检查通知权限失败:", err);
    return false;
  }
}

/** 发送桌面新邮件通知 */
export async function sendNewMailNotification(
  accountName: string,
  addedCount: number,
  firstMail?: MailSummary,
): Promise<void> {
  if (addedCount <= 0) return;
  const content = formatNewMailNotification(accountName, addedCount, firstMail);
  if (!content.title) return;

  try {
    const ok = await ensureNotificationPermission();
    if (!ok) return;

    sendNotification({
      title: content.title,
      body: content.body,
    });
  } catch (err) {
    console.warn("发送系统通知失败:", err);
  }
}
