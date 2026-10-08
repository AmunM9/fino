/** The studio behind Fino. Names stay as they are in every language. */
export const studio = {
  name: "The Shipping Labs",
  founded: 2026,
  /** With the trailing slash: the opener scope (capabilities/default.json) matches "…com/*". */
  url: "https://theshippinglabs.com/",
  host: "theshippinglabs.com",
} as const;

/** Same line as the bundle's copyright (src-tauri/tauri.conf.json), shown in About. */
export const copyright = `© ${studio.founded} ${studio.name}`;
