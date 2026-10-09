/** 线性图标（风格参考 SF Symbols），颜色跟随 currentColor */
const PATHS = {
  inbox: "M3.5 13.5 6 5.5A2 2 0 0 1 7.9 4h8.2A2 2 0 0 1 18 5.5l2.5 8M3.5 13.5V18a2 2 0 0 0 2 2h13a2 2 0 0 0 2-2v-4.5M3.5 13.5h4.3a1 1 0 0 1 .9.6l.6 1.3a1 1 0 0 0 .9.6h3.6a1 1 0 0 0 .9-.6l.6-1.3a1 1 0 0 1 .9-.6h4.3",
  tray: "M4 7.5h16M4 7.5V18a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5M4 7.5 5.5 4.5h13L20 7.5M9.5 11.5h5",
  gear: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.6-2-3.4-2.4.9a7.5 7.5 0 0 0-2.6-1.5L14 2.5h-4l-.4 2.4A7.5 7.5 0 0 0 7 6.4l-2.4-.9-2 3.4 2 1.6a7.6 7.6 0 0 0 0 3l-2 1.6 2 3.4 2.4-.9a7.5 7.5 0 0 0 2.6 1.5l.4 2.4h4l.4-2.4a7.5 7.5 0 0 0 2.6-1.5l2.4.9 2-3.4-2-1.6Z",
  plus: "M12 5v14M5 12h14",
  refresh: "M20 11.5A8 8 0 1 0 17.7 17M20 5v6.5h-6.5",
  compose: "M14.5 5.5 18.5 9.5M4 20l1-4.5L15.8 4.7a1.8 1.8 0 0 1 2.5 0l1 1a1.8 1.8 0 0 1 0 2.5L8.5 19 4 20Z",
  checkAll: "M2.5 12.5 6.5 16.5 14 8M10.5 15l1.5 1.5L21.5 7",
  envelope: "M3.5 6.5h17v11a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5v-11ZM3.5 6.5 12 13l8.5-6.5",
  envelopeOpen: "M3.5 10 12 4.5l8.5 5.5V18.5A1.5 1.5 0 0 1 19 20H5a1.5 1.5 0 0 1-1.5-1.5V10ZM3.5 10l8.5 6 8.5-6",
  chevronLeft: "M15 5l-7 7 7 7",
  chevronUp: "M6 15l6-6 6 6",
  chevronRight: "M9 5l7 7-7 7",
  sidebar: "M5 4.5h14a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-11a2 2 0 0 1 2-2ZM9.5 4.5v15M5.5 8h1.5M5.5 11h1.5",
  chevronDown: "M6 9l6 6 6-6",
  paperclip: "M20 11.5 12.2 19.3a5 5 0 0 1-7-7l8-8a3.3 3.3 0 0 1 4.7 4.7L10 16.9a1.7 1.7 0 0 1-2.4-2.4l7.4-7.4",
  shield: "M12 3 4.5 6v5.5c0 4.6 3.2 8.4 7.5 9.5 4.3-1.1 7.5-4.9 7.5-9.5V6L12 3Z",
  moon: "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z",
  sun: "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4",
  warning: "M12 4 2.8 19.5h18.4L12 4ZM12 10v4.5M12 17v.01",
  mail: "M3.5 6.5h17v11a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5v-11ZM3.5 6.5 12 13l8.5-6.5",
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 16, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      className={`icon ${className ?? ""}`}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
