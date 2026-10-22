// Tauri 的 beforeBundleCommand：打包前把命令行程序合成通用版（universal）。
//
// 通用版 macOS 构建时，Tauri 分别编出 Apple Silicon 与 Intel 两份，再用 lipo 合成
// universal——但只合成主程序。同一个包里的命令行程序 skill-studio-cli 也会被打进 .app，
// 打包器到 target/universal-apple-darwin/<profile>/ 找它，所以要在这里先合成。缺了它
// 打包器会直接报错，不会打出少了命令行的安装包。其他平台什么也不做。
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const triple = process.env.TAURI_ENV_TARGET_TRIPLE;
if (triple === "universal-apple-darwin") {
  const profile = process.env.TAURI_ENV_DEBUG === "true" ? "debug" : "release";
  const target = process.env.CARGO_TARGET_DIR ?? "target";
  const parts = ["aarch64-apple-darwin", "x86_64-apple-darwin"].map((arch) =>
    join(target, arch, profile, "skill-studio-cli"),
  );
  for (const part of parts)
    if (!existsSync(part)) throw new Error(`缺少 ${part}，无法合成通用版命令行程序`);
  const dir = join(target, "universal-apple-darwin", profile);
  mkdirSync(dir, { recursive: true });
  const out = join(dir, "skill-studio-cli");
  execFileSync("lipo", ["-create", "-output", out, ...parts], { stdio: "inherit" });
  console.log(`已合成通用版命令行程序：${out}`);
}
