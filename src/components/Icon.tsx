const paths = {
  moreVertical:
    "M12 4a1 1 0 1 0 0 2 1 1 0 0 0 0-2ZM12 11a1 1 0 1 0 0 2 1 1 0 0 0 0-2ZM12 18a1 1 0 1 0 0 2 1 1 0 0 0 0-2Z",
  save: "M4 3h13l4 4v14H3V3h1Zm3 0v7h10V3M7 21v-7h10v7",
  languages: "M3 5h12M9 3v2M5 5c1 5 4 8 9 10M13 5c-1 5-4 8-9 10M14 21l4-10 4 10M16 17h4",
  grid: "M3 3h7v7H3V3Zm11 0h7v7h-7V3ZM3 14h7v7H3v-7Zm11 0h7v7h-7v-7Z",
  chevronDown: "M6 9l6 6 6-6",
  paragraph: "M13 4h7M13 4v16M18 4v16M13 4H9a4 4 0 0 0 0 8h4",
  richText: "M5 3h10l4 4v14H5V3Zm10 0v4h4M8 11h8M8 15h8M8 18h5",
  sparkles:
    "M12 3l2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3ZM20 3v4M18 5h4",
  format: "M4 5h12M10 5v14M6 19h8M17 13l4 8M19 17h4",
  book: "M4 4h6a3 3 0 0 1 2 1 3 3 0 0 1 2-1h6v15h-6a3 3 0 0 0-2 1 3 3 0 0 0-2-1H4V4Zm8 1v15",
  trash: "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7",
  plus: "M12 5v14M5 12h14",
  minus: "M5 12h14",
  search: "M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z",
  upload: "M12 16V3M7 8l5-5 5 5M4 15v6h16v-6",
  download: "M12 3v13M7 11l5 5 5-5M4 17v4h16v-4",
  left: "M15 5l-7 7 7 7",
  right: "M9 5l7 7-7 7",
  back: "M19 12H5M12 19l-7-7 7-7",
  sidebar: "M3 4h18v16H3V4Zm6 0v16",
  check: "M5 12l4 4L19 6",
  copy: "M9 9h12v12H9V9ZM15 9V3H3v12h6",
  keep: "M5 4h14v16H5V4Zm4 5h6M9 13h6",
  sun: "M12 8a4 4 0 1 1 0 8 4 4 0 0 1 0-8ZM12 2v2M12 20v2M2 12h2M20 12h2M5 5l1 1M18 18l1 1M5 19l1-1M18 6l1-1",
  moon: "M21 13A9 9 0 1 1 11 3a7 7 0 0 0 10 10Z",
  monitor: "M3 3h18v14H3V3Zm9 14v4M8 21h8",
  bold: "M6 4h7a4 4 0 0 1 0 8H6V4Zm0 8h8a4 4 0 0 1 0 8H6v-8Z",
  italic: "M10 4h9M5 20h9M15 4 9 20",
  underline: "M6 3v8a6 6 0 0 0 12 0V3M4 21h16",
  highlight: "M5 16l9-12 6 5-9 12-6-5ZM5 16l-2 5h8M3 23h18M12 7l6 5",
  superscript: "M3 19 8 5l5 14M5 14h6M17 3h2v7M16 10h5",
  subscript: "M3 19 8 5l5 14M5 14h6M16 15c0-3 5-3 5 0 0 2-5 3-5 6h5",
  link: "M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2",
  unlink:
    "M10 13l4-4M7 17l-1 1a3 3 0 0 1-4-4l4-4a3 3 0 0 1 4 0M17 7l1-1a3 3 0 0 1 4 4l-4 4a3 3 0 0 1-4 0M3 3l18 18",
  strike: "M17 5c-2-3-10-2-10 2 0 2 2 3 5 4M4 12h16M7 19c3 3 10 2 10-2 0-1-1-2-3-3",
  undo: "M9 5 4 10l5 5M4 10h10a6 6 0 0 1 6 6v3",
  redo: "M15 5l5 5-5 5M20 10H10a6 6 0 0 0-6 6v3",
  image: "M3 3h18v18H3V3Zm0 14 6-6 5 5 3-3 4 4M16 7h.01",
  list: "M9 5h12M9 12h12M9 19h12M3 5h1M3 12h1M3 19h1",
  numbered: "M10 5h11M10 12h11M10 19h11M3 3h2v5M3 10h3v2l-3 3h3M3 18h3v4H3",
  code: "M8 6 2 12l6 6M16 6l6 6-6 6M14 3l-4 18",
  blockquote: "M5 6h5v6H5V6Zm0 6c0 4 2 5 4 5M14 6h5v6h-5V6Zm0 6c0 4 2 5 4 5",
  codeBlock: "M3 3h18v18H3V3Zm5 5-4 4 4 4M16 8l4 4-4 4",
  edit: "M4 20h4L20 8l-4-4L4 16v4ZM14 6l4 4",
  rowAdd: "M3 3h18v11H3V3Zm0 6h18M12 17v6M9 20h6",
  columnAdd: "M3 3h11v18H3V3Zm6 0v18M17 12h6M20 9v6",
  rowRemove: "M3 3h18v11H3V3Zm0 6h18M9 20h6",
  columnRemove: "M3 3h11v18H3V3Zm6 0v18M17 12h6",
  table: "M3 3h18v18H3V3Zm0 6h18M3 15h18M9 3v18M15 3v18",
  close: "M6 6l12 12M6 18 18 6",
  settings: "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6",
  scrollSync:
    "M3 4h6v16H3V4ZM15 4h6v16h-6V4ZM12 7v10M12 7L10 9M12 7l2 2M12 17l-2-2M12 17l2-2",
  refresh:
    "M23 4v6h-6M1 20v-6h6M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15",
  minimize: "M4 4h16v3H4V4Zm4 6h8v6H8v-6Zm-4 6h16v4H4v-4Z",
  panelOriginal: "M3 4h18v16H3V4Zm9 0v16M5.5 8h4M5.5 11h4M5.5 14h3",
  panelTranslation: "M3 4h18v16H3V4Zm9 0v16M14.5 8h4M14.5 11h4M14.5 14h3",
  expand: "M8 3H3v5M16 3h5v5M3 16v5h5M21 16v5h-5",
} as const;
export type IconName = keyof typeof paths;
export function Icon({
  name,
  size = 18,
  spin = false,
}: {
  name: IconName;
  size?: number;
  spin?: boolean;
}) {
  return (
    <svg
      className={spin ? "icon icon--spin" : "icon"}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d={paths[name]} />
    </svg>
  );
}
