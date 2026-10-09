import { api, type BadgeUpdateParams } from "./api";

/** 徽标文字格式化纯函数 */
export function formatBadgeLabel(count: number): string {
  if (count <= 0) return "";
  if (count > 99) return "99+";
  return String(count);
}

let cachedBaseIcon: HTMLImageElement | null = null;

function loadBaseIcon(): Promise<HTMLImageElement | null> {
  if (typeof window === "undefined" || typeof Image === "undefined") {
    return Promise.resolve(null);
  }
  if (cachedBaseIcon && cachedBaseIcon.complete && cachedBaseIcon.naturalWidth > 0) {
    return Promise.resolve(cachedBaseIcon);
  }
  return new Promise((resolve) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.src = "/icon.png";
    img.onload = () => {
      cachedBaseIcon = img;
      resolve(img);
    };
    img.onerror = () => {
      resolve(null);
    };
  });
}

/** 绘制 32x32 的 Windows 任务栏 Overlay 图标（圆形红底白字） */
export function renderOverlayIcon(count: number): Uint8ClampedArray | null {
  if (count <= 0 || typeof document === "undefined") return null;
  const label = formatBadgeLabel(count);
  const size = 32;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;

  ctx.clearRect(0, 0, size, size);

  // 红色圆形背景
  ctx.fillStyle = "#ef4444";
  ctx.beginPath();
  ctx.arc(size / 2, size / 2, size / 2 - 1, 0, 2 * Math.PI);
  ctx.fill();

  // 1px 白色描边
  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = 1.5;
  ctx.stroke();

  // 白色文字
  ctx.fillStyle = "#ffffff";
  ctx.font = label.length > 2 ? "bold 13px sans-serif" : "bold 17px sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(label, size / 2, size / 2 + 1);

  return ctx.getImageData(0, 0, size, size).data;
}

/** 绘制 128x128 的主图标并在右上角叠加红底白字数字角标 */
export async function renderWindowIcon(count: number): Promise<Uint8ClampedArray | null> {
  if (count <= 0 || typeof document === "undefined") return null;
  const label = formatBadgeLabel(count);
  const size = 128;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;

  ctx.clearRect(0, 0, size, size);

  const base = await loadBaseIcon();
  if (base && base.naturalWidth > 0) {
    ctx.drawImage(base, 0, 0, size, size);
  } else {
    // 降级：绘制默认圆角蓝色信封背景
    ctx.fillStyle = "#0284c7";
    ctx.beginPath();
    ctx.roundRect(8, 8, size - 16, size - 16, 24);
    ctx.fill();
  }

  // 在右上角绘制徽章
  // 徽章大小：单数字圆形，多数字胶囊
  const isCapsule = label.length > 1;
  const badgeHeight = 44;
  const badgeWidth = isCapsule ? Math.max(52, label.length * 18 + 16) : 44;
  const badgeX = size - badgeWidth - 4;
  const badgeY = 4;
  const radius = badgeHeight / 2;

  // 阴影
  ctx.save();
  ctx.shadowColor = "rgba(0, 0, 0, 0.45)";
  ctx.shadowBlur = 6;
  ctx.shadowOffsetX = 0;
  ctx.shadowOffsetY = 2;

  // 红色胶囊/圆形
  ctx.fillStyle = "#dc2626";
  ctx.beginPath();
  ctx.roundRect(badgeX, badgeY, badgeWidth, badgeHeight, radius);
  ctx.fill();
  ctx.restore();

  // 白色描边
  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = 3.5;
  ctx.beginPath();
  ctx.roundRect(badgeX, badgeY, badgeWidth, badgeHeight, radius);
  ctx.stroke();

  // 白色加粗文本
  ctx.fillStyle = "#ffffff";
  ctx.font = label.length > 2 ? "bold 22px sans-serif" : "bold 26px sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(label, badgeX + badgeWidth / 2, badgeY + badgeHeight / 2 + 1);

  return ctx.getImageData(0, 0, size, size).data;
}

let lastBadgeCount: number | null = null;

/** 更新应用任务栏角标与图标（包含 Windows Overlay 与主窗口右上角徽章） */
export async function updateAppBadge(count: number): Promise<void> {
  const safeCount = Math.max(0, count);
  if (lastBadgeCount === safeCount) return;
  lastBadgeCount = safeCount;

  if (safeCount === 0) {
    await api.updateBadge({ count: 0 }).catch(() => {});
    return;
  }

  try {
    const overlayData = renderOverlayIcon(safeCount);
    const windowData = await renderWindowIcon(safeCount);

    const payload: BadgeUpdateParams = {
      count: safeCount,
      overlayRgba: overlayData ? Array.from(overlayData) : undefined,
      overlaySize: overlayData ? 32 : undefined,
      iconRgba: windowData ? Array.from(windowData) : undefined,
      iconSize: windowData ? 128 : undefined,
    };
    await api.updateBadge(payload);
  } catch (err) {
    console.warn("更新应用角标失败:", err);
  }
}
