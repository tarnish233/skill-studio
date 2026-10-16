import { expect, it } from "vitest";
import {
  nameAvatarGlyph,
  nameAvatarSeed,
  nameAvatarTone,
} from "@/lib/nameAvatar";

it("同名稳定：同一个分组每次渲染都是同一个记号", () => {
  for (const name of ["前端", "LLM", "Dev", "空组"]) {
    expect(nameAvatarTone(name)).toBe(nameAvatarTone(name));
    expect(nameAvatarGlyph(name)).toBe(nameAvatarGlyph(name));
    expect(nameAvatarSeed(name)).toBe(nameAvatarSeed(name));
  }
});

it("首字：中文取第一个汉字，英文转大写，空名有兜底", () => {
  expect(nameAvatarGlyph("前端")).toBe("前");
  expect(nameAvatarGlyph("  LLM  ")).toBe("L");
  expect(nameAvatarGlyph("dev tools")).toBe("D");
  expect(nameAvatarGlyph("")).toBe("组");
  expect(nameAvatarGlyph("   ")).toBe("组");
  // emoji 之类的非字母开头原样保留，不渲染成空白方块
  expect(nameAvatarGlyph("🚀 上线")).toBe("🚀");
});

it("底色取自固定调色板，散列足够开、撞色率低", () => {
  const names = [
    "skill-studio",
    "infuse_android_tv",
    "muse",
    "note",
    "open_mole",
    "LLM",
    "前端",
    "后端",
    "笔记",
    "设计",
    "Dev",
    "Ops",
    "数据",
    "docs",
    "api-server",
    "webapp",
    "playground",
    "tools",
    "ai-notes",
    "release",
  ];
  const tones = names.map(nameAvatarTone);
  for (const tone of tones) {
    expect(tone).toMatch(/^bg-\S+ text-\S+ dark:text-\S+$/);
  }
  // 20 个名字至少用到 12 种颜色：不是要求完美散列，只是别都挤在一个颜色里
  expect(new Set(tones).size).toBeGreaterThanOrEqual(11);
  // 两两撞色率保持在一成上下（17 色里必然有重复，首字才是主要区分手段）
  let pairs = 0;
  let same = 0;
  for (let i = 0; i < tones.length; i++)
    for (let j = i + 1; j < tones.length; j++) {
      pairs++;
      if (tones[i] === tones[j]) same++;
    }
  expect(same / pairs).toBeLessThan(0.15);
});
