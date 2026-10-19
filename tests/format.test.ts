import { expect, it } from "vitest";
import { formatBytes } from "@/lib/format";

it("formats sizes with one decimal below 10 and whole numbers above", () => {
  expect(formatBytes(0)).toBe("0 B");
  expect(formatBytes(1023)).toBe("1023 B");
  expect(formatBytes(1024)).toBe("1.0 KB");
  expect(formatBytes(3_500)).toBe("3.4 KB");
  expect(formatBytes(871_424)).toBe("851 KB");
  expect(formatBytes(1_258_291)).toBe("1.2 MB");
  expect(formatBytes(5 * 1024 ** 4)).toBe("5.0 TB");
});

it("moves to the next unit when rounding would show 1024, and never writes 10.0", () => {
  // 1023.999 KB 取整就是「1024 KB」，该写成 MB 了
  expect(formatBytes(1024 ** 2 - 1)).toBe("1.0 MB");
  expect(formatBytes(1024 ** 3 - 1)).toBe("1.0 GB");
  // 9.95 起一位小数会写成「10.0」，和 10 以上取整的写法不一致
  expect(formatBytes(10_189)).toBe("10 KB");
  expect(formatBytes(10_188)).toBe("9.9 KB");
});
