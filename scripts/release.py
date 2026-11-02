#!/usr/bin/env python3
"""Skill Studio release checks and tag publishing (Python 3.11+, git, gh)."""
import argparse
import hashlib
import json
import re
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]
REPO = "tarnish233/skill-studio"
PACKAGES = {
    "skill-studio",
    "skill-studio-cli",
    "skill-studio-core",
    "skill-studio-mcp",
    "skill-studio-remote",
    "skill-studio-service",
}


def run(*args, timeout=None):
    return subprocess.check_output(args, cwd=ROOT, text=True, timeout=timeout).strip()


def gh(*args):
    return json.loads(run("gh", *args, "--repo", REPO, timeout=120))


def version(value):
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", value):
        raise ValueError("版本必须是 x.y.z，不含 v")
    return value


def workspace_packages(root):
    """工作区里每个成员的包名：新加的 crate 没进 PACKAGES 时，set-version 不会改它的锁版本，
    CI 的 --locked 构建就会失败，所以在这里先拦下"""
    members = tomllib.loads((root / "Cargo.toml").read_text()).get("workspace", {}).get("members", [])
    return {tomllib.loads((root / member / "Cargo.toml").read_text())["package"]["name"] for member in members}


def versions(root):
    missing = workspace_packages(root) - PACKAGES
    if missing:
        raise ValueError(f"PACKAGES 缺少工作区成员：{', '.join(sorted(missing))}")
    values = {
        file: json.loads((root / file).read_text())["version"]
        for file in ("package.json", "src-tauri/tauri.conf.json")
    }
    values["Cargo.toml"] = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    packages = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    for name in PACKAGES:
        found = [p["version"] for p in packages if p["name"] == name]
        if len(found) != 1:
            raise ValueError(f"Cargo.lock 必须恰有一个 {name}")
        values[name] = found[0]
    return values


def notes(root, number):
    text = (root / "CHANGELOG.md").read_text()
    sections = list(re.finditer(r"^## v([^\s]+)[^\n]*\n", text, re.M))
    matching = [s for s in sections if s[1] == number]
    if len(matching) != 1:
        raise ValueError(f"CHANGELOG.md 必须包含且仅包含一个 v{number} 小节")
    start = matching[0].end()
    end = next((s.start() for s in sections if s.start() >= start), len(text))
    body = text[start:end].strip()
    if not body or re.search(r"\b(?:TODO|TBD)\b", body, re.I):
        raise ValueError("发布说明为空或尚未完成")
    return body


def verify(root=ROOT, tag=None):
    values = versions(root)
    number = version(values["package.json"])
    if set(values.values()) != {number}:
        raise ValueError(f"版本不一致：{values}")
    if tag is not None and tag != f"v{number}":
        raise ValueError(f"标签 {tag} 与版本 {number} 不一致")
    notes(root, number)
    return number


def set_version(root, number):
    version(number)
    # Parse all inputs before changing files; do not rewrite third-party lock versions.
    versions(root)
    changed = {}
    for file in ("package.json", "src-tauri/tauri.conf.json"):
        text = (root / file).read_text()
        changed[file], count = re.subn(r'("version"\s*:\s*")[^"]+(")', lambda m: m[1] + number + m[2], text, count=1)
        if count != 1:
            raise ValueError(f"无法更新 {file} 版本")
    text = (root / "Cargo.toml").read_text()
    changed["Cargo.toml"], count = re.subn(
        r'(\[workspace\.package\][\s\S]*?^version\s*=\s*")[^"]+("[^\n]*)',
        lambda m: m[1] + number + m[2], text, count=1, flags=re.M,
    )
    if count != 1:
        raise ValueError("找不到 workspace.package.version")
    text = (root / "Cargo.lock").read_text()
    for name in PACKAGES:
        text, count = re.subn(
            rf'(\[\[package\]\]\nname = "{re.escape(name)}"\nversion = ")[^"]+("\n)',
            lambda m: m[1] + number + m[2], text,
        )
        if count != 1:
            raise ValueError(f"无法更新 {name} 锁版本")
    changed["Cargo.lock"] = text
    for file, text in changed.items():
        (root / file).write_text(text)


def latest_run(runs, sha, branch, event="push"):
    matches = [r for r in runs if r["headSha"] == sha and r["headBranch"] == branch and r["event"] == event]
    return max(matches, key=lambda r: r["databaseId"]) if matches else None


def successful_run(runs, sha, branch, event="push"):
    latest = latest_run(runs, sha, branch, event)
    if latest is None:
        raise ValueError(f"未找到 {branch}@{sha[:7]} 的 {event} 流水线")
    if latest["status"] != "completed" or latest["conclusion"] != "success":
        raise ValueError(f"流水线尚未成功：{latest['url']} ({latest['status']}/{latest['conclusion']})")
    return latest


def workflow_runs(workflow, branch):
    return gh(
        "run", "list", "--workflow", workflow, "--branch", branch,
        "--event", "push", "--limit", "100", "--json",
        "databaseId,headSha,headBranch,event,status,conclusion,url",
    )


def workflow_run(workflow, sha, branch):
    return successful_run(workflow_runs(workflow, branch), sha, branch)


def wait_workflow(workflow, sha, branch, timeout=3600):
    """Poll inside one process; emit only discovery/completion, never job logs."""
    deadline = time.monotonic() + timeout
    discovery_deadline = min(deadline, time.monotonic() + 300)
    current = None
    print(f"等待 {workflow}：{branch}@{sha[:7]}", flush=True)
    while True:
        if current is None:
            current = latest_run(workflow_runs(workflow, branch), sha, branch)
            if current is not None:
                print(f"流水线 {current['databaseId']}：{current['url']}", flush=True)
        else:
            current = gh("run", "view", str(current["databaseId"]), "--json",
                         "databaseId,headSha,headBranch,event,status,conclusion,url")
        if current is not None and current["status"] == "completed":
            result = successful_run([current], sha, branch)
            print(f"{workflow} 已成功", flush=True)
            return result
        limit = deadline if current is not None else discovery_deadline
        if time.monotonic() >= limit:
            location = current["url"] if current else f"{branch}@{sha[:7]}（流水线未出现）"
            raise ValueError(f"等待超时：{location}；未取消远程任务，可重新运行同一命令续跑")
        time.sleep(min(60, max(0, limit - time.monotonic())))


def ci_gate(sha):
    return workflow_run("ci.yml", sha, "main")


def publication_context(number):
    verify(tag=f"v{number}")
    if run("git", "branch", "--show-current") != "main":
        raise ValueError("只允许从 main 发布")
    if run("git", "status", "--porcelain", "--untracked-files=no"):
        raise ValueError("先提交已审查的改动；脚本不会代为暂存或提交文件")
    remote = run("git", "remote", "get-url", "origin")
    if remote not in (f"git@github.com:{REPO}.git", f"https://github.com/{REPO}.git", f"https://github.com/{REPO}"):
        raise ValueError("origin 不是 Skill Studio 仓库")
    run("git", "fetch", "origin", "main", "--tags")
    sha = run("git", "rev-parse", "HEAD")
    if sha != run("git", "rev-parse", "origin/main"):
        raise ValueError("HEAD 与远程 main 不一致，先处理并推送 main")
    return sha


def publish_tag(number, wait=False, timeout=3600):
    sha = publication_context(number)
    if wait:
        wait_workflow("ci.yml", sha, "main", timeout)
        # A long wait must not allow a different checkout or remote HEAD to be tagged.
        if publication_context(number) != sha:
            raise ValueError("等待期间 HEAD 已变化，停止发布")
    else:
        ci_gate(sha)
    tag = f"v{number}"
    existing = run("git", "tag", "--list", tag)
    if existing:
        if run("git", "rev-parse", f"{tag}^{{commit}}") != sha:
            raise ValueError("标签已指向其他提交，禁止覆盖")
    else:
        run("git", "tag", "-a", tag, "-m", f"Skill Studio {tag}")
    # Retrying the same tag push is harmless; never delete or force-update tags.
    run("git", "push", "origin", f"refs/tags/{tag}")
    print(f"已推送 {tag} ({sha})", flush=True)
    return sha


def macos_asset_suffix(number):
    # v0.6.18 is the last published Universal release. Keep historical verification
    # valid without allowing Universal/Intel assets in subsequent releases.
    return "aarch64.dmg" if tuple(map(int, version(number).split("."))) >= (0, 6, 19) else "universal.dmg"


def check_assets(assets, number):
    suffixes = {macos_asset_suffix(number), "x64-setup.exe", "x64_en-US.msi", "amd64.AppImage", "amd64.deb"}
    found = set()
    for asset in assets:
        match = re.fullmatch(rf"Skill[. ]Studio_{re.escape(number)}_(.+)", asset["name"])
        if not match or match[1] not in suffixes or asset["size"] <= 0 or match[1] in found:
            raise ValueError(f"安装包异常或版本不符：{asset['name']}")
        found.add(match[1])
    if found != suffixes:
        raise ValueError(f"缺少安装包：{sorted(suffixes - found)}")


def download_hash(url):
    digest = hashlib.sha256()
    with urlopen(url, timeout=60) as response:
        for chunk in iter(lambda: response.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def with_cask_cli(text):
    app = '  app "Skill Studio.app"\n'
    binary = '  binary "#{appdir}/Skill Studio.app/Contents/MacOS/skill-studio-cli", target: "skill-studio"\n'
    if binary not in text:
        if text.count(app) != 1:
            raise ValueError("Cask 必须恰有一个应用入口")
        text = text.replace(app, app + binary, 1)
    return text


def update_cask(text, release, hash_download):
    """Validate a stable DMG before updating only the Cask's owned fields."""
    if release.get("draft") or release.get("prerelease"):
        return text
    tag = release["tag_name"]
    number = version(tag.removeprefix("v"))
    if tag != f"v{number}":
        raise ValueError(f"不支持的发布标签：{tag}")
    current = re.findall(r'^  version "([^"]+)"$', text, re.M)
    if len(current) != 1:
        raise ValueError("Cask 必须恰有一个版本")
    current_number = version(current[0])
    if tuple(map(int, number.split("."))) <= tuple(map(int, current_number.split("."))):
        return text
    suffix = macos_asset_suffix(number)
    name = f"Skill.Studio_{number}_{suffix}"
    assets = [a for a in release["assets"] if a["name"] == name]
    if len(assets) != 1 or assets[0].get("state") != "uploaded" or assets[0].get("size", 0) <= 0:
        raise ValueError(f"发布 DMG 未就绪：{name}")
    asset = assets[0]
    url = f"https://github.com/{REPO}/releases/download/{tag}/{name}"
    if asset["browser_download_url"] != url:
        raise ValueError("发布资产 URL 不匹配")
    digest = hash_download(url)
    if not re.fullmatch(r"[a-f0-9]{64}", digest):
        raise ValueError("下载的 SHA-256 无效")
    if asset.get("digest") and asset["digest"] != f"sha256:{digest}":
        raise ValueError("下载的 DMG 与 GitHub 摘要不匹配")
    text, count = re.subn(r'^  sha256 "[a-f0-9]{64}"$', f'  sha256 "{digest}"', text, flags=re.M)
    if count != 1:
        raise ValueError("Cask 必须恰有一个 SHA-256")
    urls = re.findall(r'^  url "([^"]+)"$', text, re.M)
    prefix = f"https://github.com/{REPO}/releases/download/v#{{version}}/Skill.Studio_#{{version}}_"
    if len(urls) != 1 or urls[0] not in {prefix + "universal.dmg", prefix + "aarch64.dmg"}:
        raise ValueError("Cask 下载 URL 不匹配")
    text = text.replace(f'  url "{urls[0]}"', f'  url "{prefix}{suffix}"', 1)
    text = re.sub(r'^  version "[^"]+"$', f'  version "{number}"', text, flags=re.M)
    if suffix == "aarch64.dmg":
        arch = re.findall(r'^  depends_on arch:.*$', text, re.M)
        if len(arch) > 1:
            raise ValueError("Cask 架构限制重复")
        if arch:
            text = text.replace(arch[0], "  depends_on arch: :arm64", 1)
        else:
            macos = re.findall(r'^  depends_on macos:.*$', text, re.M)
            if len(macos) != 1:
                raise ValueError("Cask 必须恰有一个 macOS 版本限制")
            text = text.replace(macos[0], "  depends_on arch: :arm64\n" + macos[0], 1)
    return with_cask_cli(text)


def update_homebrew_cask(path, number):
    version(number)
    tag = f"v{number}"
    data = json.loads(run("gh", "api", f"repos/{REPO}/releases/tags/{tag}", timeout=120))
    if data["tag_name"] != tag or data["draft"] or data["prerelease"]:
        raise ValueError("需要指定版本的正式发布")
    if not data["assets"] and data.get("id"):
        data["assets"] = json.loads(run("gh", "api", f"repos/{REPO}/releases/{data['id']}/assets", timeout=120))
    previous = path.read_text()
    # Keep the existing workflow's CLI-link repair even on an already-current Cask.
    updated = with_cask_cli(update_cask(previous, data, download_hash))
    if updated == previous:
        print("Cask is already current.")
        return
    path.write_text(updated)
    print(f"Updated Cask to {tag}")


def publish_assets(directory):
    number = verify()
    tag = f"v{number}"
    files = sorted(p for p in directory.rglob("*") if p.is_file())
    check_assets([{"name": p.name, "size": p.stat().st_size} for p in files], number)
    # A failed read is not evidence that a release is absent.
    releases = gh("release", "list", "--limit", "1000", "--json", "tagName")
    if not any(r["tagName"] == tag for r in releases):
        run("gh", "release", "create", tag, "--repo", REPO, "--verify-tag", "--draft",
            "--title", f"Skill Studio {tag}", "--notes", notes(ROOT, number))
    data = gh("release", "view", tag, "--json", "isDraft,isPrerelease,assets")
    if data["isPrerelease"]:
        raise ValueError("不覆盖预发布版本")
    if data["isDraft"]:
        run("gh", "release", "upload", tag, *map(str, files), "--repo", REPO, "--clobber")
        data = gh("release", "view", tag, "--json", "assets")
        check_assets(data["assets"], number)
        run("gh", "release", "edit", tag, "--repo", REPO, "--draft=false",
            "--notes", notes(ROOT, number))
    else:
        # Retrying a published release only verifies it, never replaces its assets.
        check_assets(data["assets"], number)


def verify_release(number):
    tag = f"v{number}"
    sha = run("git", "rev-parse", f"{tag}^{{commit}}")
    ci_gate(sha)
    workflow_run("release.yml", sha, tag)  # Includes Homebrew update.
    data = gh("release", "view", tag, "--json", "tagName,isDraft,isPrerelease,assets,url")
    if data["tagName"] != tag or data["isDraft"] or data["isPrerelease"]:
        raise ValueError("尚未正式发布")
    check_assets(data["assets"], number)
    return data["url"]


def release(number, timeout=3600):
    sha = publish_tag(number, wait=True, timeout=timeout)
    wait_workflow("release.yml", sha, f"v{number}", timeout)
    # One final independent check covers CI, Homebrew and all five assets.
    print(verify_release(number), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("set-version", "publish-tag", "verify-release"):
        sub.add_parser(command).add_argument("version", type=version)
    release_parser = sub.add_parser("release", help="等待 CI、推标签、等待发布并最终核验")
    release_parser.add_argument("version", type=version)
    release_parser.add_argument("--timeout", type=int, default=3600, help="每条流水线最多等待秒数（默认 3600）")
    sub.add_parser("verify").add_argument("--tag")
    sub.add_parser("check")
    sub.add_parser("ci-gate").add_argument("--sha", required=True)
    sub.add_parser("notes").add_argument("--output", type=Path, required=True)
    artifacts = sub.add_parser("artifacts")
    artifacts.add_argument("directory", type=Path)
    sub.add_parser("publish-assets").add_argument("directory", type=Path)
    cask = sub.add_parser("update-cask", help="核验指定发布的 DMG 并更新 Homebrew Cask")
    cask.add_argument("file", type=Path)
    cask.add_argument("--version", type=version, required=True)
    args = parser.parse_args()
    if args.command == "release":
        if args.timeout <= 0:
            parser.error("--timeout 必须为正数")
        release(args.version, args.timeout)
    elif args.command == "set-version":
        set_version(ROOT, args.version)
    elif args.command == "update-cask":
        update_homebrew_cask(args.file, args.version)
    elif args.command == "verify":
        print(verify(tag=args.tag))
    elif args.command == "check":
        verify()
        for command in (
            ["python3", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_release.py"],
            ["pnpm", "typecheck"], ["pnpm", "format:check"], ["pnpm", "test:unit"], ["pnpm", "build:renderer"],
            ["cargo", "fmt", "--all", "--check"], ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
            ["cargo", "test", "--workspace"],
        ):
            subprocess.run(command, cwd=ROOT, check=True)
    elif args.command == "ci-gate":
        print(ci_gate(args.sha)["url"])
    elif args.command == "publish-tag":
        publish_tag(args.version)
    elif args.command == "publish-assets":
        publish_assets(args.directory)
    elif args.command == "notes":
        args.output.write_text(notes(ROOT, verify()) + "\n")
    elif args.command == "artifacts":
        check_assets([{"name": p.name, "size": p.stat().st_size} for p in args.directory.rglob("*") if p.is_file()], verify())
    elif args.command == "verify-release":
        print(verify_release(args.version))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as error:
        print(f"发布检查失败：{error}", file=sys.stderr)
        sys.exit(1)
