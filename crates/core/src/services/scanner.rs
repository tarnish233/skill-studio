//! 扫描 skill 目录、解析 `SKILL.md` frontmatter、计算内容哈希。

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::models::agent::AgentDescriptor;

/// 复制模式的元数据边车文件名。
///
/// 复制出来的目录没有回溯到源的信息，无法判断是否过期，也无法区分"我们复制的"
/// 和"用户自己放的"。这个文件就是那条回溯线索。
pub const COPY_SIDECAR: &str = ".skill-studio-copy.json";

/// `SKILL.md` 是识别一个 skill 目录的唯一锚点
pub const SKILL_FILE: &str = "SKILL.md";

/// Agent Skills 通用 frontmatter 字段。额外字段仅作为扩展信息展示，
/// 不能据此判定某个 Agent 不兼容，或推断整个插件的安装能力。
const PORTABLE_KEYS: &[&str] = &[
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
];

/// 目录项的分类
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EntryKind {
    /// 真目录，且含 SKILL.md —— 可以作为 skill 真身
    RealSkill,
    /// 符号链接（或 Windows junction），指向别处
    #[serde(rename_all = "camelCase")]
    Link { target: Option<PathBuf> },
    /// 有复制边车，说明是本工具复制过来的
    #[serde(rename_all = "camelCase")]
    ManagedCopy {
        source_path: PathBuf,
        source_hash: String,
    },
    /// 真目录但没有 SKILL.md —— 不是 skill，扫描时忽略
    NotASkill,
}

/// 扫描到的一个目录项
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedEntry {
    pub name: String,
    pub path: PathBuf,
    pub root: PathBuf,
    pub agent_id: String,
    pub kind: EntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontmatter: Option<Frontmatter>,
}

/// `SKILL.md` 的 frontmatter
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    /// 通用字段集合之外的扩展字段；具体语义和兼容性取决于目标 Agent。
    #[serde(default)]
    pub extra_keys: Vec<String>,
    /// frontmatter 存在但 YAML 解析失败
    #[serde(default)]
    pub malformed: bool,
    #[serde(default)]
    pub error: Option<String>,
}

/// 复制边车的内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopySidecar {
    pub skill_id: String,
    pub source_path: PathBuf,
    pub source_hash: String,
    pub copied_at: i64,
}

/// `SKILL.md` 的结构化预览：原文、frontmatter、去掉 frontmatter 的正文。
///
/// 预览要展示的是正文，不能把 YAML 头当成两行 `---` 一起渲染；解析规则与扫描
/// 共用 [`frontmatter_parts`]，避免预览和列表页对同一份文件给出两种结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDocument {
    /// 原始文件内容（含 BOM），供需要对照或复制的调用方使用
    pub raw: String,
    /// frontmatter 解析结果；没有 frontmatter 时为默认值
    pub frontmatter: Frontmatter,
    /// 去掉 frontmatter 块之后的正文
    pub body: String,
}

/// frontmatter 的解析结果与它在原文中的边界
struct FrontmatterParts {
    frontmatter: Frontmatter,
    /// 正文起始字节；`None` 表示没有可剥离的 frontmatter 块
    body_start: Option<usize>,
}

/// 解析 `SKILL.md` 的 frontmatter。
///
/// 容错到底：读不到、没有 frontmatter、YAML 坏掉，都不报错 —— 一个坏 skill
/// 不该让整次扫描失败，调用方会回落到目录名。
pub fn parse_frontmatter(skill_md: &Path) -> Frontmatter {
    match fs::read_to_string(skill_md) {
        Ok(raw) => frontmatter_parts(&raw).frontmatter,
        Err(_) => Frontmatter::default(),
    }
}

/// 拆出 frontmatter 与正文起点。
fn frontmatter_parts(raw: &str) -> FrontmatterParts {
    // 某些编辑器会写 BOM；frontmatter 只有在 `---` 位于**首行**时才生效
    let content = raw.trim_start_matches('\u{feff}');
    let bom = raw.len() - content.len();
    let missing_fence = || FrontmatterParts {
        frontmatter: Frontmatter {
            malformed: true,
            error: Some("frontmatter 缺少结束标记 ---".into()),
            ..Default::default()
        },
        // 没有闭合标记就说不清正文从哪开始，整篇照原样展示，由调用方给出提示
        body_start: None,
    };
    if content.lines().next().map(str::trim_end) != Some("---") {
        return FrontmatterParts {
            frontmatter: Frontmatter::default(),
            body_start: None,
        };
    }
    // 首个 `---` 之后再找闭合的 `---`
    let rest = &content[3..];
    let Some(end) = find_closing_fence(rest) else {
        return missing_fence();
    };
    // 闭合围栏整行都属于 frontmatter，不能只跳到行首
    let closing = rest[end..].split_inclusive('\n').next().map_or(0, str::len);
    FrontmatterParts {
        frontmatter: parse_frontmatter_block(&rest[..end]),
        body_start: Some(bom + 3 + end + closing),
    }
}

/// 解析 frontmatter 块本身（不含两行 `---`）
fn parse_frontmatter_block(block: &str) -> Frontmatter {
    match serde_yaml::from_str::<BTreeMap<String, serde_yaml::Value>>(block) {
        Ok(map) => {
            let as_string = |key: &str| -> Option<String> {
                map.get(key).and_then(|v| match v {
                    serde_yaml::Value::String(s) if !s.trim().is_empty() => {
                        Some(s.trim().to_string())
                    }
                    _ => None,
                })
            };
            let extra_keys = map
                .keys()
                .filter(|k| !PORTABLE_KEYS.contains(&k.as_str()))
                .cloned()
                .collect();
            Frontmatter {
                name: as_string("name"),
                description: as_string("description"),
                extra_keys,
                malformed: false,
                error: None,
            }
        }
        Err(error) => Frontmatter {
            malformed: true,
            error: Some(error.to_string()),
            ..Default::default()
        },
    }
}

/// 找 frontmatter 的闭合围栏：必须独占一行
fn find_closing_fence(rest: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']).trim() == "---" && offset > 0 {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// 计算目录内容哈希（SHA-256 前 32 位十六进制）。
///
/// 用于复制模式的 drift 检测。规则：
/// - 按相对路径排序，保证跨平台结果一致
/// - 路径和内容长度都进哈希，避免拼接歧义
/// - 跳过复制边车自身（它含哈希，会自我循环）
/// - 符号链接只记录目标，不遍历；内部目标使用相对路径，迁移不改变哈希
pub fn dir_content_hash(dir: &Path) -> Result<String> {
    content_hash(dir, false)
}

pub(crate) fn backup_content_hash(dir: &Path) -> Result<String> {
    content_hash(dir, true)
}

fn content_hash(dir: &Path, literal_links: bool) -> Result<String> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files, 0)?;
    files.sort();

    let mut hasher = Sha256::new();
    for rel in &files {
        let full = dir.join(rel);
        let link_bytes = if is_symlink_or_junction(&full) {
            // Domain separation prevents a regular file containing the link's
            // textual representation from being mistaken for the same entry.
            hasher.update(b"symlink\0");
            let target = if literal_links {
                fs::read_link(&full).map_err(|e| Error::io(&full, e))?
            } else {
                link_destination(&full)?
            };
            let key = if literal_links {
                format!("literal:{}", target.display())
            } else {
                match target.strip_prefix(crate::fs::paths::normalize_path_lexically(dir)) {
                    Ok(rel) => format!("internal:{}", rel.display()),
                    Err(_) => format!("external:{}", target.display()),
                }
            };
            Some(format!("symlink:{key}").into_bytes())
        } else {
            None
        };
        let rel_key = rel.to_string_lossy().replace('\\', "/");
        hasher.update((rel_key.len() as u64).to_le_bytes());
        hasher.update(rel_key.as_bytes());
        if let Some(bytes) = link_bytes {
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        } else {
            let mut file = fs::File::open(&full).map_err(|e| Error::io(&full, e))?;
            let len = file.metadata().map_err(|e| Error::io(&full, e))?.len();
            hasher.update(len.to_le_bytes());
            let mut buffer = [0u8; 64 * 1024];
            let mut read = 0u64;
            loop {
                let n = match file.read(&mut buffer) {
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(Error::io(&full, e)),
                };
                if n == 0 {
                    break;
                }
                read += n as u64;
                hasher.update(&buffer[..n]);
            }
            if read != len {
                return Err(Error::invalid("计算哈希期间文件大小发生变化"));
            }
        }
    }
    let digest = hasher.finalize();
    Ok(digest.iter().take(16).map(|b| format!("{b:02x}")).collect())
}

/// 目录遍历的深度上限：够深的 references 树，又不至于被恶意深目录（或跟错的链接）
/// 拖住。哈希、token 估算、复制树走的是三条遍历，但**只该有一个上限** —— 各写一份
/// 字面量的时候，改了一处就会静悄悄地留下两种深度。
pub const MAX_SCAN_DEPTH: usize = 16;

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > MAX_SCAN_DEPTH {
        return Err(Error::invalid(format!("目录层级过深: {}", dir.display())));
    }
    let entries = fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let path = entry.path();
        // 链接只记录目标，禁止递归跟随。
        let meta = path.symlink_metadata().map_err(|e| Error::io(&path, e))?;
        if is_symlink_or_junction(&path) {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
            continue;
        }
        if meta.is_dir() {
            collect_files(root, &path, out, depth + 1)?;
        } else if meta.is_file() {
            if dir == root && path.file_name().is_some_and(|n| n == COPY_SIDECAR) {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        }
    }
    Ok(())
}

/// Resolve a link lexically, without traversing it (also handles dangling links).
pub fn link_destination(path: &Path) -> Result<PathBuf> {
    let target = fs::read_link(path).map_err(|e| Error::io(path, e))?;
    Ok(crate::fs::paths::normalize_path_lexically(
        &if target.is_absolute() {
            target
        } else {
            path.parent().unwrap().join(target)
        },
    ))
}

/// 是否为符号链接或 Windows junction。
///
/// `mklink /J` 建的 junction **不会**被 Rust 的 `FileType::is_symlink()` 报告
/// （cc-switch 就漏了这一点），Windows 上要额外查 reparse point 属性。
pub fn is_symlink_or_junction(path: &Path) -> bool {
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    // 块尾表达式而非 return —— cfg 剥离后它就是函数尾表达式，
    // 写 return 会在 Windows 上触发 clippy::needless_return。
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

/// 读取复制边车
pub fn read_copy_sidecar(dir: &Path) -> Option<CopySidecar> {
    let raw = fs::read_to_string(dir.join(COPY_SIDECAR)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 分类一个目录项
pub fn classify_entry(path: &Path) -> EntryKind {
    if is_symlink_or_junction(path) {
        return EntryKind::Link {
            target: fs::read_link(path).ok(),
        };
    }
    if !path.is_dir() {
        return EntryKind::NotASkill;
    }
    if let Some(sidecar) = read_copy_sidecar(path) {
        return EntryKind::ManagedCopy {
            source_path: sidecar.source_path,
            source_hash: sidecar.source_hash,
        };
    }
    if path.join(SKILL_FILE).is_file() {
        EntryKind::RealSkill
    } else {
        EntryKind::NotASkill
    }
}

/// 扫描一个全局 / 项目 skill 根目录。根目录不存在时返回空列表，不报错。
pub fn scan_root(root: &Path, agent: &AgentDescriptor) -> Result<Vec<ScannedEntry>> {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(Error::io(root, err)),
    };

    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(root, e))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        // Claude Code 的 `synced` 是 claude.ai 同步专用保留名，必须跳过
        if agent.is_reserved_dir(&name) {
            log::debug!("跳过保留目录 {name}（{} 专用）", agent.id);
            continue;
        }
        let path = entry.path();
        let kind = classify_entry(&path);
        if matches!(kind, EntryKind::NotASkill) {
            continue;
        }
        let frontmatter = {
            let skill_md = path.join(SKILL_FILE);
            if skill_md.is_file() {
                Some(parse_frontmatter(&skill_md))
            } else {
                None
            }
        };
        out.push(ScannedEntry {
            name,
            path,
            root: root.to_path_buf(),
            agent_id: agent.id.to_string(),
            kind,
            frontmatter,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Read project-local installations without adopting them or changing bindings.
pub fn scan_project(root: &Path) -> Result<Vec<ScannedEntry>> {
    let mut entries = Vec::new();
    for agent in crate::models::agent::AGENTS {
        if let Some(dir) = agent.project_root(root) {
            entries.extend(scan_root(&dir, agent)?);
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    Ok(entries)
}

/// 源目录是否可以作为同步源。
///
/// **不变式：源必须含 `SKILL.md`。** 空的源目录绝不能去替换目标 —— 否则会把
/// 用户 agent 里的真 skill 抹掉。cc-switch 为此专门写了回归测试。
pub fn validate_sync_source(source: &Path) -> Result<()> {
    if !source.is_dir() {
        return Err(Error::invalid(format!("源不是目录: {}", source.display())));
    }
    if !source.join(SKILL_FILE).is_file() {
        return Err(Error::invalid(format!(
            "源目录缺少 {SKILL_FILE}，拒绝同步以避免清空目标: {}",
            source.display()
        )));
    }
    Ok(())
}

fn strip_line_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// Native Harness accepts only exact `---` fence lines (LF/CRLF); BOM, padded and
/// indented fences are ignored, and the first exact closing fence wins.
fn harness_frontmatter(raw: &str) -> Result<&str> {
    let (opening, rest) = raw.split_once('\n').unwrap_or((raw, ""));
    if strip_line_eol(opening) != "---" {
        return Err(Error::invalid(
            "DeepSeek Harness Skill 必须包含 YAML frontmatter 的 name 和 description",
        ));
    }
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if strip_line_eol(line) == "---" {
            return Ok(&rest[..offset]);
        }
        offset += line.len();
    }
    Err(Error::invalid(
        "DeepSeek Harness Skill 的 frontmatter 缺少结束标记",
    ))
}

/// Check the target Agent's actual filesystem-provider contract before planning writes.
/// Harness frontmatter is stricter than the generic catalog; never rewrite invocation policy.
pub fn validate_agent_sync_source(source: &Path, agent: &AgentDescriptor) -> Result<()> {
    validate_sync_source(source)?;
    if agent.id != "deepseek-harness" {
        return Ok(());
    }
    let fields: BTreeMap<String, serde_yaml::Value> =
        serde_yaml::from_str(harness_frontmatter(&read_skill_raw(source)?)?)
            .map_err(|_| Error::invalid("DeepSeek Harness Skill 的 YAML frontmatter 无效"))?;
    let name = fields.get("name").and_then(serde_yaml::Value::as_str);
    if !name.is_some_and(|name| {
        name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
    }) {
        return Err(Error::invalid("DeepSeek Harness Skill 的 name 必须是小写字母/数字组成的 kebab-case，不能有空格、连续或首尾连字符"));
    }
    if !fields
        .get("description")
        .and_then(serde_yaml::Value::as_str)
        .is_some_and(|text| !text.is_empty())
    {
        return Err(Error::invalid(
            "DeepSeek Harness Skill 的 description 必须是非空字符串",
        ));
    }
    for key in ["disableModelInvocation", "modelInvocable", "userInvocable"] {
        if fields.contains_key(key) {
            return Err(Error::invalid(format!("DeepSeek Harness 不接受 frontmatter 字段 {key}，请使用 disable-model-invocation / user-invocable")));
        }
    }
    for key in ["disable-model-invocation", "user-invocable"] {
        if let Some(value) = fields.get(key) {
            let valid = match value {
                serde_yaml::Value::Bool(_) => true,
                serde_yaml::Value::Number(n) => matches!(n.as_f64(), Some(0.0 | 1.0)),
                serde_yaml::Value::String(text) => matches!(
                    text.to_ascii_lowercase().as_str(),
                    "true" | "false" | "yes" | "no" | "on" | "off" | "0" | "1"
                ),
                _ => false,
            };
            if !valid {
                return Err(Error::invalid(format!("DeepSeek Harness Skill 的 {key} 必须是布尔值（true/false、yes/no、on/off、0/1）")));
            }
        }
    }
    Ok(())
}

/// 预览与 DeepSeek Harness 预检共用的读取上限。
const SKILL_DOCUMENT_LIMIT: u64 = 1024 * 1024;

/// 有界读取 `SKILL.md`：普通文件、不超过 1 MiB、有效 UTF-8；预览与预检共用。
fn read_skill_raw(directory: &Path) -> Result<String> {
    let path = directory.join(SKILL_FILE);
    let metadata = fs::metadata(&path).map_err(|e| Error::io(&path, e))?;
    if !metadata.is_file() {
        return Err(Error::invalid("SKILL.md 不是普通文件"));
    }
    if metadata.len() > SKILL_DOCUMENT_LIMIT {
        return Err(Error::invalid("SKILL.md 超过 1 MB，请在编辑器中打开"));
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|e| Error::io(&path, e))?
        .take(SKILL_DOCUMENT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::io(&path, e))?;
    if bytes.len() > SKILL_DOCUMENT_LIMIT as usize {
        return Err(Error::invalid("SKILL.md 超过预览大小限制"));
    }
    String::from_utf8(bytes).map_err(|_| Error::invalid("SKILL.md 不是有效的 UTF-8 文本"))
}

/// Bounded preview: raw text plus frontmatter split, never rendered or executed.
pub fn read_skill_document(directory: &Path) -> Result<SkillDocument> {
    let raw = read_skill_raw(directory)?;
    let parts = frontmatter_parts(&raw);
    let body = match parts.body_start {
        Some(start) => raw
            .get(start..)
            .unwrap_or_default()
            .trim_start_matches(['\r', '\n'])
            .to_string(),
        // 无法确定 frontmatter 边界时按整篇展示（BOM 会让正文多出一个不可见字符）
        None => raw.trim_start_matches('\u{feff}').to_string(),
    };
    Ok(SkillDocument {
        raw,
        frontmatter: parts.frontmatter,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::agent::find_agent;

    fn write_skill(dir: &Path, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(SKILL_FILE), body).unwrap();
    }

    #[test]
    fn parses_name_and_description() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("pdf");
        write_skill(
            &s,
            "---\nname: pdf-tools\ndescription: 处理 PDF\n---\n\n正文",
        );
        let fm = parse_frontmatter(&s.join(SKILL_FILE));
        assert_eq!(fm.name.as_deref(), Some("pdf-tools"));
        assert_eq!(fm.description.as_deref(), Some("处理 PDF"));
        assert!(fm.extra_keys.is_empty());
        assert!(!fm.malformed);
    }

    #[test]
    fn frontmatter_only_counts_when_fence_is_first_line() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        // 前面有一行正文，则 --- 不构成 frontmatter
        write_skill(&s, "# 标题\n---\nname: nope\n---\n");
        let fm = parse_frontmatter(&s.join(SKILL_FILE));
        assert!(fm.name.is_none());
    }

    #[test]
    fn strips_utf8_bom_before_checking_fence() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        write_skill(&s, "\u{feff}---\nname: bom-ok\n---\n");
        assert_eq!(
            parse_frontmatter(&s.join(SKILL_FILE)).name.as_deref(),
            Some("bom-ok")
        );
    }

    #[test]
    fn collects_non_portable_keys() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        // context / agent / model 是 Claude Code 专有；allowed-tools 属可移植集合
        write_skill(
            &s,
            "---\nname: x\ndescription: d\ncontext: fork\nagent: Explore\nmodel: inherit\nallowed-tools: Read\n---\n",
        );
        let fm = parse_frontmatter(&s.join(SKILL_FILE));
        let mut extra = fm.extra_keys.clone();
        extra.sort();
        assert_eq!(extra, vec!["agent", "context", "model"]);
    }

    #[test]
    fn malformed_yaml_is_flagged_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        write_skill(&s, "---\nname: [unclosed\n---\n");
        let fm = parse_frontmatter(&s.join(SKILL_FILE));
        assert!(fm.malformed);
        assert!(fm.name.is_none());
    }

    #[test]
    fn missing_file_yields_empty_frontmatter() {
        let fm = parse_frontmatter(Path::new("/definitely/not/here/SKILL.md"));
        assert_eq!(fm, Frontmatter::default());
    }

    #[test]
    fn preview_splits_frontmatter_from_body() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        let raw = "---\nname: x\ndescription: d\ncontext: fork\n---\n\n# 标题\n正文";
        write_skill(&s, raw);
        let doc = read_skill_document(&s).unwrap();
        assert_eq!(doc.raw, raw);
        assert_eq!(doc.frontmatter.name.as_deref(), Some("x"));
        assert_eq!(doc.frontmatter.extra_keys, vec!["context"]);
        assert_eq!(doc.body, "# 标题\n正文");
    }

    #[test]
    fn preview_without_frontmatter_keeps_whole_document() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        write_skill(&s, "\u{feff}# 只有标题\n");
        let doc = read_skill_document(&s).unwrap();
        assert_eq!(doc.frontmatter, Frontmatter::default());
        assert_eq!(doc.body, "# 只有标题\n");
        assert_eq!(doc.raw, "\u{feff}# 只有标题\n");
    }

    #[test]
    fn preview_keeps_unclosed_fence_with_its_warning() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        let raw = "---\nname: x\n\n正文";
        write_skill(&s, raw);
        let doc = read_skill_document(&s).unwrap();
        assert!(doc.frontmatter.malformed);
        assert_eq!(doc.body, raw);
    }

    #[test]
    fn preview_strips_empty_frontmatter_block() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        write_skill(&s, "---\n---\n正文\n");
        let doc = read_skill_document(&s).unwrap();
        assert_eq!(doc.frontmatter, Frontmatter::default());
        assert_eq!(doc.body, "正文\n");
    }

    #[test]
    fn preview_document_is_the_bounded_raw_reader_plus_generic_frontmatter() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("x");
        write_skill(
            &s,
            "\u{feff}---\nname: demo\ndescription: d\ncontext: fork\n---\n\n正文",
        );
        let raw = read_skill_raw(&s).unwrap();
        let doc = read_skill_document(&s).unwrap();
        let parts = frontmatter_parts(&raw);
        assert_eq!(doc.raw, raw);
        assert_eq!(doc.frontmatter, parts.frontmatter);
        assert_eq!(doc.frontmatter.extra_keys, vec!["context"]);
        assert_eq!(doc.body, "正文");
    }

    #[test]
    fn harness_preflight_requires_exact_fences_while_generic_preview_tolerates_bom() {
        let d = tempfile::tempdir().unwrap();
        let harness = crate::models::agent::require_agent("deepseek-harness").unwrap();
        let check = |content: &str| {
            fs::write(d.path().join(SKILL_FILE), content).unwrap();
            validate_agent_sync_source(d.path(), harness)
        };
        for content in [
            "\u{feff}---\nname: demo\ndescription: test\n---\nbody\n",
            "--- \nname: demo\ndescription: test\n---\nbody\n",
            "---\nname: demo\ndescription: test\n--- \nbody\n",
            "---\nname: demo\ndescription: test\n  ---\nbody\n",
            "---\n---\nname: demo\ndescription: test\n---\nbody\n",
        ] {
            assert!(
                check(content).is_err(),
                "native Harness rejects {content:?}"
            );
        }
        assert!(check("---\r\nname: demo\r\ndescription: test\r\n---\r\nbody\r\n").is_ok());
        fs::write(
            d.path().join(SKILL_FILE),
            "\u{feff}---\nname: demo\ndescription: test\n---\nbody\n",
        )
        .unwrap();
        let generic = crate::models::agent::require_agent("opencode").unwrap();
        assert!(validate_agent_sync_source(d.path(), generic).is_ok());
        let document = read_skill_document(d.path()).unwrap();
        assert_eq!(document.frontmatter.name.as_deref(), Some("demo"));
        assert_eq!(document.body.trim(), "body");
    }

    #[test]
    fn preflight_and_preview_share_the_bounded_raw_read_contract() {
        let d = tempfile::tempdir().unwrap();
        let s = d.path().join("demo");
        fs::create_dir_all(&s).unwrap();
        let harness = crate::models::agent::require_agent("deepseek-harness").unwrap();
        let header = "---\nname: demo\ndescription: test\n---\n";
        let exact = format!("{header}{}", "x".repeat(1024 * 1024 - header.len()));
        fs::write(s.join(SKILL_FILE), &exact).unwrap();
        assert_eq!(read_skill_document(&s).unwrap().raw, exact);
        assert!(validate_agent_sync_source(&s, harness).is_ok());
        for content in [format!("{exact}x").into_bytes(), vec![0xff]] {
            fs::write(s.join(SKILL_FILE), content).unwrap();
            let preview = read_skill_document(&s).unwrap_err();
            let preflight = validate_agent_sync_source(&s, harness).unwrap_err();
            assert_eq!(preflight.to_string(), preview.to_string());
        }
        fs::remove_file(s.join(SKILL_FILE)).unwrap();
        fs::create_dir(s.join(SKILL_FILE)).unwrap();
        assert!(read_skill_document(&s).is_err());
        assert!(validate_agent_sync_source(&s, harness).is_err());
    }

    #[test]
    fn content_hash_is_stable_and_content_sensitive() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        write_skill(&a, "---\nname: a\n---\nbody");
        let h1 = dir_content_hash(&a).unwrap();
        let h2 = dir_content_hash(&a).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32);

        fs::write(a.join(SKILL_FILE), "---\nname: a\n---\nchanged").unwrap();
        assert_ne!(dir_content_hash(&a).unwrap(), h1);
    }

    #[test]
    fn content_hash_ignores_the_copy_sidecar() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        write_skill(&a, "---\nname: a\n---\n");
        let before = dir_content_hash(&a).unwrap();
        fs::write(a.join(COPY_SIDECAR), r#"{"skillId":"x"}"#).unwrap();
        assert_eq!(dir_content_hash(&a).unwrap(), before, "边车不应影响哈希");
    }

    #[test]
    fn content_hash_covers_nested_files() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        write_skill(&a, "---\nname: a\n---\n");
        let before = dir_content_hash(&a).unwrap();
        fs::create_dir_all(a.join("scripts")).unwrap();
        fs::write(a.join("scripts/run.sh"), "echo hi").unwrap();
        assert_ne!(dir_content_hash(&a).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn content_hash_tracks_symlinks_and_survives_cycles() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        write_skill(&a, "---\nname: a\n---\n");
        let before = dir_content_hash(&a).unwrap();
        // 自指环：不跳过 symlink 就会栈溢出
        std::os::unix::fs::symlink(&a, a.join("loop")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", a.join("outside")).unwrap();
        assert_ne!(dir_content_hash(&a).unwrap(), before);
    }

    #[test]
    fn validate_sync_source_rejects_dir_without_skill_md() {
        let d = tempfile::tempdir().unwrap();
        let empty = d.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let err = validate_sync_source(&empty).unwrap_err().to_string();
        assert!(err.contains(SKILL_FILE), "{err}");

        let ok = d.path().join("ok");
        write_skill(&ok, "---\nname: ok\n---\n");
        assert!(validate_sync_source(&ok).is_ok());
    }

    #[test]
    fn scan_root_returns_empty_for_missing_dir() {
        let claude = find_agent("claude-code").unwrap();
        let out = scan_root(Path::new("/definitely/not/here"), claude).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn scan_root_skips_reserved_synced_dir_for_claude() {
        let d = tempfile::tempdir().unwrap();
        write_skill(&d.path().join("real"), "---\nname: real\n---\n");
        write_skill(&d.path().join("synced"), "---\nname: synced\n---\n");
        write_skill(&d.path().join("Synced-Extra"), "---\nname: x\n---\n");

        let claude = find_agent("claude-code").unwrap();
        let names: Vec<_> = scan_root(d.path(), claude)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(names.contains(&"real".to_string()));
        assert!(!names.contains(&"synced".to_string()), "synced 必须跳过");
        // 只跳过精确同名，不是前缀匹配
        assert!(names.contains(&"Synced-Extra".to_string()));

        // Codex 没有保留名，synced 会被正常扫到
        let codex = find_agent("codex").unwrap();
        let codex_names: Vec<_> = scan_root(d.path(), codex)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(codex_names.contains(&"synced".to_string()));
    }

    #[test]
    fn scan_root_ignores_dirs_without_skill_md_and_dotfiles() {
        let d = tempfile::tempdir().unwrap();
        write_skill(&d.path().join("good"), "---\nname: good\n---\n");
        fs::create_dir_all(d.path().join("not-a-skill")).unwrap();
        fs::create_dir_all(d.path().join(".hidden")).unwrap();
        fs::write(d.path().join("loose.md"), "x").unwrap();

        let claude = find_agent("claude-code").unwrap();
        let entries = scan_root(d.path(), claude).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "good");
        assert_eq!(entries[0].kind, EntryKind::RealSkill);
    }

    #[cfg(unix)]
    #[test]
    fn scan_root_classifies_symlinks_as_links() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        write_skill(&src, "---\nname: src\n---\n");
        let root = d.path().join("root");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(&src, root.join("linked")).unwrap();

        let claude = find_agent("claude-code").unwrap();
        let entries = scan_root(&root, claude).unwrap();
        assert_eq!(entries.len(), 1);
        match &entries[0].kind {
            EntryKind::Link { target } => {
                assert_eq!(target.as_deref(), Some(src.as_path()));
            }
            other => panic!("应识别为 Link，实际 {other:?}"),
        }
    }

    #[test]
    fn classify_detects_managed_copy_via_sidecar() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        write_skill(&a, "---\nname: a\n---\n");
        let sidecar = CopySidecar {
            skill_id: "abc".into(),
            source_path: PathBuf::from("/src/a"),
            source_hash: "deadbeef".into(),
            copied_at: 1,
        };
        fs::write(
            a.join(COPY_SIDECAR),
            serde_json::to_string(&sidecar).unwrap(),
        )
        .unwrap();

        match classify_entry(&a) {
            EntryKind::ManagedCopy {
                source_path,
                source_hash,
            } => {
                assert_eq!(source_path, PathBuf::from("/src/a"));
                assert_eq!(source_hash, "deadbeef");
            }
            other => panic!("应识别为 ManagedCopy，实际 {other:?}"),
        }
    }
}
