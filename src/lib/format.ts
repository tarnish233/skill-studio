/** 文件大小：`812 B`、`3.4 KB`、`851 KB`、`1.2 MB` */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  // 按显示出来的数进位：1023.6 KB 取整就是「1024 KB」，该换成 MB 了
  while (value >= 1023.5 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  // 9.95 起一位小数会写成「10.0」，和 10 以上取整的写法对齐
  return `${value >= 9.95 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}
