/**
 * 列表卡片的图标：不存图标，按名字确定性地生成「底色 + 首字」。
 *
 * 分组和项目以前是「所有卡片共用同一个 Layers / 文件夹图标」，多张卡片摊在
 * 一起分不清哪个是哪个。改成从名字算出来的标记后，每张卡片自带一个稳定的、
 * 互不相同的记号：同一条目每次渲染都一样，改了名字就换一个。
 *
 * 颜色只用固定的整串 class（Tailwind 需要静态字符串才能生成），按名字哈希取；
 * 首字取名字的第一个字符，中文用汉字、英文转大写，避免出现全角/半角混排。
 */

/** 底色 / 前景色成对出现，浅色和深色主题各一套。17 个色相：少了撞色，
 * 多了相邻色相也还分得清（都是 15% 淡底）。 */
const TONES = [
  "bg-sky-500/15 text-sky-700 dark:text-sky-300",
  "bg-blue-500/15 text-blue-700 dark:text-blue-300",
  "bg-indigo-500/15 text-indigo-700 dark:text-indigo-300",
  "bg-violet-500/15 text-violet-700 dark:text-violet-300",
  "bg-purple-500/15 text-purple-700 dark:text-purple-300",
  "bg-fuchsia-500/15 text-fuchsia-700 dark:text-fuchsia-300",
  "bg-pink-500/15 text-pink-700 dark:text-pink-300",
  "bg-rose-500/15 text-rose-700 dark:text-rose-300",
  "bg-red-500/15 text-red-700 dark:text-red-300",
  "bg-orange-500/15 text-orange-700 dark:text-orange-300",
  "bg-amber-500/15 text-amber-700 dark:text-amber-300",
  "bg-yellow-500/15 text-yellow-700 dark:text-yellow-300",
  "bg-lime-500/15 text-lime-700 dark:text-lime-300",
  "bg-green-500/15 text-green-700 dark:text-green-300",
  "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
  "bg-teal-500/15 text-teal-700 dark:text-teal-300",
  "bg-cyan-500/15 text-cyan-700 dark:text-cyan-300",
] as const;

/** FNV-1a + 一级混淆：短名字的低位不够匀，混询一下才不会全挤在少数几个颜色里 */
function mix(hash: number): number {
  let value = hash;
  value ^= value >>> 16;
  value = Math.imul(value, 0x7feb352d) >>> 0;
  value ^= value >>> 15;
  value = Math.imul(value, 0x846ca68b) >>> 0;
  value ^= value >>> 16;
  return value >>> 0;
}

export function nameAvatarSeed(name: string): number {
  let hash = 0x811c9dc5;
  for (const char of name.trim()) {
    hash ^= char.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return mix(hash);
}

export function nameAvatarTone(name: string): string {
  return TONES[nameAvatarSeed(name) % TONES.length];
}

/** 名字为空、只有一个 emoji 之类的输入都不能渲染出空白方块 */
export function nameAvatarGlyph(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return "组";
  const first = Array.from(trimmed)[0];
  return /[a-z]/.test(first) ? first.toUpperCase() : first;
}
