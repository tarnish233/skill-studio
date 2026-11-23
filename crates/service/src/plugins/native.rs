use super::*;
use jsonc_parser::cst::{CstInputValue, CstRootNode};
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use skill_studio_core::{models::agent::require_agent, services::detector};
use skill_studio_mcp::native::{cst_value, unique_properties};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

const OPERATION_TIMEOUT: Duration = Duration::from_secs(180);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_LIMIT: usize = 64 * 1024;

pub(super) fn capability(config: &AppConfig, agent: &str) -> PluginCapability {
    let mut result = PluginCapability {
        agent_id: agent.into(),
        cli_version: None,
        install: false,
        reason: None,
    };
    let fail = if !matches!(agent, "claude-code" | "opencode" | "pi") {
        Some("该 Agent 的插件首版只读".into())
    } else if config.settings.disabled_agents.iter().any(|id| id == agent) {
        Some("该 Agent 已关闭管理".into())
    } else {
        None
    };
    if fail.is_some() {
        result.reason = fail;
        return result;
    }
    let descriptor = require_agent(agent).unwrap();
    let Some(program) = detector::find_cli(descriptor.cli_command) else {
        result.reason = Some("未找到原生 CLI，插件只读".into());
        return result;
    };
    let root = descriptor.resolved_config_dir(&config.settings.agent_dir_overrides);
    let version = run(
        &program,
        &["--version".into()],
        &root,
        agent,
        PROBE_TIMEOUT,
        false,
    );
    let version = version
        .ok()
        .filter(|output| output.success)
        .and_then(|output| parse_version(&String::from_utf8_lossy(&output.stdout)));
    let Some(version) = version else {
        result.reason = Some("原生 CLI 不可用或版本探测失败，插件只读".into());
        return result;
    };
    result.cli_version = Some(format!("{}.{}.{}", version.0, version.1, version.2));
    let supported = match agent {
        "claude-code" => version.0 == 2 && version >= (2, 1, 268),
        "opencode" => version.0 == 2 && version >= (2, 3, 2),
        "pi" => version.0 == 1 && version >= (1, 6, 0),
        _ => false,
    };
    if !supported {
        result.reason = Some("此 CLI 版本的插件接口尚未验证，保持只读".into());
        return result;
    }
    if agent == "opencode" {
        let expected = paths::env_dir_override("XDG_CONFIG_HOME")
            .unwrap_or_else(|| paths::home_dir().join(".config"))
            .join("opencode");
        let same = root == expected
            || root
                .canonicalize()
                .ok()
                .zip(expected.canonicalize().ok())
                .is_some_and(|(a, b)| a == b);
        if !same || inventory::config_paths(&root, agent).len() != 1 {
            result.reason = Some(
                "OpenCode 原生命令写入默认全局配置；当前目录覆盖或多配置无法安全映射，保持只读"
                    .into(),
            );
            return result;
        }
    }
    let args = if agent == "pi" {
        vec!["--help".into()]
    } else {
        vec!["plugin".into(), "--help".into()]
    };
    let help = run(&program, &args, &root, agent, PROBE_TIMEOUT, false)
        .ok()
        .filter(|output| output.success)
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let required: &[&str] = match agent {
        "claude-code" => &["install", "enable", "disable", "update", "uninstall"],
        "opencode" => &["add", "update", "remove"],
        _ => &["install", "remove", "update"],
    };
    if required.iter().any(|name| !help.contains(name)) {
        result.reason = Some("原生帮助未声明预期插件命令，保持只读".into());
        return result;
    }
    result.install = true;
    result
}

pub(super) fn repair(state: &AppState, confirmed: bool) -> Result<PluginOperationResult, String> {
    if !confirmed {
        return Err("核对待处理插件记录需要确认".into());
    }
    let (_lock, config) = state.lock_native_config().map_err(String::from)?;
    if !config.settings.manage_plugins {
        return Err("请先开启插件管理，再核对待处理记录".into());
    }
    let changed = dir(state).join("pending-toggle.json").exists();
    recover(state, &config)?;
    Ok(PluginOperationResult {
        changed,
        pending: false,
        message: if changed {
            "已核对并完成插件管理记录，原生配置、插件文件和启停未改动".into()
        } else {
            "没有待处理的插件记录".into()
        },
        warnings: Vec::new(),
        backup_path: None,
    })
}

pub(super) fn operate(
    state: &AppState,
    input: PluginOperation,
    capture_signals: bool,
) -> Result<PluginOperationResult, String> {
    if !input.confirmed {
        return Err("插件操作需明确确认；安装、启用和更新可能执行本机代码".into());
    }
    if !matches!(input.agent_id.as_str(), "claude-code" | "opencode" | "pi") {
        return Err("该 Agent 的原生插件写入不在首版范围内".into());
    }
    state.ensure_writable().map_err(String::from)?;
    let (_lock, config) = state.lock_native_config().map_err(String::from)?;
    if !config.settings.manage_plugins {
        return Err("插件管理未开启；请先开启管理，不会自动改变已有插件".into());
    }
    if config.settings.disabled_agents.contains(&input.agent_id) {
        return Err("该 Agent 已关闭管理".into());
    }
    recover(state, &config)?;
    let mut catalog = read_catalog(state)?;
    let root = inventory::root(&config, &input.agent_id)?;
    // 安装按来源选择原生包；其余动作都指向已存在的清单行。
    let found = if input.action == PluginAction::Install {
        None
    } else {
        let found = inventory::resolve(
            &config,
            &catalog,
            input.plugin_id.as_deref().ok_or("请选择插件")?,
        )?;
        if found.view.agent_id != input.agent_id || found.view.scope != "user" {
            return Err("目标 Agent 或作用域不匹配；首版仅管理个人配置".into());
        }
        if input.expected_revision.as_deref() != Some(found.view.revision.as_str()) {
            return Err("插件或配置已被外部修改，请刷新后重新确认".into());
        }
        if !found.view.actions.contains(&input.action) {
            return Err(found
                .view
                .read_only_reason
                .clone()
                .unwrap_or_else(|| "此插件不支持该原生操作".into()));
        }
        Some(found)
    };
    if input.action == PluginAction::Forget {
        let found = found.unwrap();
        catalog.paused.retain(|record| record.id != found.view.id);
        save_catalog(state, &catalog)?;
        return Ok(PluginOperationResult {
            changed: true,
            pending: false,
            message: "已移除 Studio 停用记录，原生配置和插件文件未改动".into(),
            warnings: Vec::new(),
            backup_path: None,
        });
    }
    let supported = capability(&config, &input.agent_id);
    if !supported.install {
        return Err(supported
            .reason
            .unwrap_or_else(|| "原生插件管理不可用".into()));
    }
    if matches!(input.action, PluginAction::Enable | PluginAction::Disable)
        && input.agent_id != "claude-code"
    {
        return toggle(
            state,
            &config,
            found.unwrap(),
            input.action == PluginAction::Enable,
            catalog,
            true,
        );
    }
    let source = if let Some(found) = &found {
        found.source.clone()
    } else {
        input.source.clone().ok_or("请填写原生插件来源")?
    };
    validate_source(&source)?;
    let local_path = package_local_path(&input.agent_id, &source, &root);
    let command_source = local_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.clone());
    if input.action == PluginAction::Uninstall
        && input.agent_id == "opencode"
        && local_path.is_some()
    {
        return toggle(state, &config, found.unwrap(), false, catalog, false);
    }
    if input.action == PluginAction::Install {
        let (entries, _) = inventory::scan(&config, &catalog);
        if entries.iter().any(|found| {
            found.view.agent_id == input.agent_id
                && same_source(&input.agent_id, &found.source, &source, &root)
        }) {
            return Err("此原生包已有配置或停用记录，请从详情操作，避免重复安装".into());
        }
        if input.agent_id == "claude-code"
            && (!source.contains('@') || source.contains(['/', '\\', ':']))
        {
            return Err(
                "Claude Code 安装请填写 name@marketplace；本地插件仅检查，不生成市场或转换格式"
                    .into(),
            );
        }
        if let Some(local) = &local_path {
            let package =
                skill_studio_core::services::plugins::inspect(local, Some(&input.agent_id));
            if package.error.is_some() {
                return Err("本地插件清单不可读或格式不匹配，未安装".into());
            }
            if package.format == "agent-plugins-v1" {
                return Err("首版仅识别标准包，不把它自动接入尚未验证标准兼容的 Agent".into());
            }
            if input.agent_id == "opencode" {
                if !local.is_dir() || in_auto_directory(local, &root.join("plugins")) {
                    return Err("OpenCode 本地来源须为非自动发现目录中的插件目录".into());
                }
                return install_local_opencode(state, &root, &command_source, catalog);
            }
        }
    }
    if input.action == PluginAction::Update {
        let found = found.as_ref().unwrap();
        if found.view.package.root.join(".git").exists() {
            let git = detector::find_cli("git").ok_or("无法检查 Git 插件的本地改动，未更新")?;
            let output = run(
                &git,
                &[
                    "-C".into(),
                    found.view.package.root.to_string_lossy().into_owned(),
                    "status".into(),
                    "--porcelain".into(),
                    "--untracked-files=all".into(),
                ],
                &root,
                &input.agent_id,
                PROBE_TIMEOUT,
                capture_signals,
            )?;
            if !output.success || !output.stdout.is_empty() {
                return Err(
                    "Git 插件有本地改动或无法检查，未更新；请先在原生 Agent 中核对并保存改动"
                        .into(),
                );
            }
        }
    }
    let paths = native_files(&root, &input.agent_id);
    let before = native_signature(&paths)?;
    let backup = backup(state, &paths)?;
    let program = detector::find_cli(
        require_agent(&input.agent_id)
            .map_err(String::from)?
            .cli_command,
    )
    .ok_or("原生 CLI 已不存在")?;
    let mut commands = Vec::new();
    if input.action == PluginAction::Install && input.agent_id == "claude-code" {
        if let Some(marketplace) = &input.marketplace {
            validate_source(marketplace)?;
            let marketplace_source = package_local_path("claude-code", marketplace, &root)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| marketplace.clone());
            commands.push(vec![
                "plugin".into(),
                "marketplace".into(),
                "add".into(),
                marketplace_source,
            ]);
        }
    } else if input.marketplace.is_some() {
        return Err("只有 Claude Code 安装支持显式添加市场来源".into());
    }
    commands.push(command(&input.agent_id, input.action, &command_source)?);
    for args in commands {
        let output = run(
            &program,
            &args,
            &root,
            &input.agent_id,
            OPERATION_TIMEOUT,
            capture_signals,
        )
        .map_err(|error| {
            format!(
                "{error}。配置可能已部分变更，未自动回滚；原始配置快照：{}",
                backup.display()
            )
        })?;
        if !output.success && !already_in_goal_state(&output.stdout) {
            return Err(format!("原生插件命令失败（退出码 {}）。需要交互授权时请在 Agent 中完成，Studio 不跳过确认；配置可能已部分变更，未自动回滚。快照：{}", output.code.map(|code| code.to_string()).unwrap_or_else(|| "signal".into()), backup.display()));
        }
    }
    if input.action == PluginAction::Uninstall {
        if let Some(found) = &found {
            if catalog
                .paused
                .iter()
                .any(|record| record.id == found.view.id)
            {
                catalog.paused.retain(|record| record.id != found.view.id);
                save_catalog(state, &catalog).map_err(|_| {
                    format!(
                        "原生操作已完成，但停用记录保存失败；请刷新后核对。快照：{}",
                        backup.display()
                    )
                })?;
            }
        }
    }
    let after = native_signature(&paths)?;
    let (current, warnings) = inventory::scan(&config, &catalog);
    let observed = match input.action {
        PluginAction::Install => current.iter().any(|found| {
            found.view.agent_id == input.agent_id
                && found.view.configured
                && same_source(&input.agent_id, &found.source, &source, &root)
        }),
        PluginAction::Uninstall => !current.iter().any(|found| {
            found.view.agent_id == input.agent_id && found.source == source && found.view.configured
        }),
        PluginAction::Enable | PluginAction::Disable => current.iter().any(|found| {
            found.view.agent_id == input.agent_id
                && found.source == source
                && found.view.enabled == Some(input.action == PluginAction::Enable)
        }),
        _ => true,
    };
    let changed = before != after
        || found.as_ref().is_some_and(|old| {
            current
                .iter()
                .find(|item| item.view.id == old.view.id)
                .is_some_and(|new| new.view.revision != old.view.revision)
        });
    Ok(PluginOperationResult {
        changed,
        pending: !observed,
        message: if !observed {
            "原生命令已退出，但尚未确认目标配置状态；请在 Agent 中核对".into()
        } else if input.action == PluginAction::Update && !changed {
            "原生命令已完成，未检测到版本或配置变化（可能已固定版本，或是本地包）".into()
        } else {
            "原生操作已完成并重读清单；已有 Agent 会话可能需要重载".into()
        },
        warnings,
        backup_path: Some(backup),
    })
}

fn toggle(
    state: &AppState,
    config: &AppConfig,
    found: Found,
    enabled: bool,
    mut catalog: Catalog,
    preserve_pause: bool,
) -> Result<PluginOperationResult, String> {
    if found.view.enabled == Some(enabled) {
        return Ok(PluginOperationResult {
            changed: false,
            pending: false,
            message: "插件配置已处于目标状态".into(),
            warnings: Vec::new(),
            backup_path: None,
        });
    }
    let before = read_text(&found.view.config_path)?.unwrap_or_default();
    let document = CstRootNode::parse(before.trim_start_matches('\u{feff}'), &Default::default())
        .map_err(|_| "原生 JSONC 配置无效，未写入")?;
    let object = document
        .object_value_or_create()
        .ok_or("原生配置根节点必须为对象")?;
    unique_properties(&object).map_err(|_| "原生配置包含重复字段，未猜测或覆盖")?;
    let field = found
        .field
        .as_deref()
        .unwrap_or(if found.view.agent_id == "pi" {
            "packages"
        } else {
            "plugins"
        });
    let array = object
        .array_value_or_create(field)
        .ok_or("原生插件声明不是数组，未写入")?;
    let root = inventory::root(config, &found.view.agent_id)?;
    let original = if enabled {
        found
            .paused
            .as_ref()
            .map(|record| record.original.clone())
            .unwrap_or_else(|| {
                let mut value = found.declaration.clone();
                if let Some(object) = value.as_object_mut() {
                    for key in ["extensions", "skills", "prompts", "themes"] {
                        object.remove(key);
                    }
                }
                value
            })
    } else {
        found.declaration.clone()
    };
    let applied = if found.view.agent_id == "pi" && !enabled {
        let mut value = found.declaration.as_object().cloned().unwrap_or_default();
        value.insert("source".into(), Value::String(found.source.clone()));
        for key in ["extensions", "skills", "prompts", "themes"] {
            value.insert(key.into(), serde_json::json!([]));
        }
        Value::Object(value)
    } else {
        Value::Null
    };
    if let Some(index) = found.index {
        let elements = array.elements();
        let node = elements.get(index).ok_or("插件声明已变更，请刷新")?;
        let current =
            jsonc_parser::parse_to_serde_value::<Value>(&node.to_string(), &Default::default())
                .map_err(|_| "插件声明不可读")?;
        if current != found.declaration {
            return Err("插件声明已被外部修改，未覆盖".into());
        }
        node.clone().remove();
        if found.view.agent_id == "pi" {
            array.insert(index, cst_value(if enabled { &original } else { &applied }));
        }
    } else {
        if found.view.agent_id != "opencode" || !enabled {
            return Err("原生声明已被移除，未重建".into());
        }
        let record = found.paused.as_ref().ok_or("缺少可恢复的原生声明")?;
        let doc = config_document(&found.view.config_path, "opencode")?;
        // 新旧两个键都可能是同一包的声明，任一存在都不重建，避免重复。
        if ["plugins", "plugin"].iter().any(|field| {
            doc[*field].as_array().is_some_and(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| source_of(entry, "opencode"))
                    .any(|source| {
                        source_identity("opencode", source, &root)
                            == source_identity("opencode", &record.source, &root)
                    })
            })
        }) {
            return Err("原生配置已有同一包，未插入旧声明".into());
        }
        array.insert(
            record.index.min(array.elements().len()),
            cst_value(&original),
        );
    }
    catalog.paused.retain(|record| record.id != found.view.id);
    if !enabled && preserve_pause {
        catalog.paused.push(PausedPlugin {
            id: found.view.id,
            agent: found.view.agent_id,
            config_path: found.view.config_path.clone(),
            source: found.source,
            original,
            applied,
            index: found.index.unwrap(),
            package_root: (!found.view.package.root.as_os_str().is_empty())
                .then_some(found.view.package.root),
            field: found.field.clone(),
        });
    }
    commit(
        state,
        &found.view.config_path,
        &before,
        &document.to_string(),
        &catalog,
    )
}

#[derive(Serialize, Deserialize)]
struct Pending {
    path: PathBuf,
    before_hash: String,
    after_hash: String,
    catalog_before_hash: String,
    catalog_after: Catalog,
}

fn commit(
    state: &AppState,
    path: &Path,
    before: &str,
    after: &str,
    catalog: &Catalog,
) -> Result<PluginOperationResult, String> {
    let pending_path = dir(state).join("pending-toggle.json");
    let catalog_path = dir(state).join("catalog.json");
    let catalog_before = read_text(&catalog_path)?.unwrap_or_default();
    let pending = Pending {
        path: path.into(),
        before_hash: digest(before.as_bytes()),
        after_hash: digest(after.as_bytes()),
        catalog_before_hash: digest(catalog_before.as_bytes()),
        catalog_after: catalog.clone(),
    };
    let backup = backup(state, &[path.into()])?;
    private_json(&pending_path, &pending)?;
    if read_text(path)?.unwrap_or_default() != before {
        let _ = fs::remove_file(&pending_path);
        return Err("原生配置已被外部修改，未覆盖".into());
    }
    atomic::atomic_write_private(path, after.as_bytes())
        .map_err(|_| "原生配置写入失败；停用记录待检查")?;
    save_catalog(state, catalog)
        .map_err(|_| "原生配置已写入，但管理记录未保存；请刷新并在下次操作时检查恢复")?;
    fs::remove_file(&pending_path).map_err(|_| "配置和记录已写入，但待处理标记清理失败")?;
    Ok(PluginOperationResult {
        changed: true,
        pending: false,
        message: "已更新个人配置，包文件和数据未移动；Agent 可能需要重载".into(),
        warnings: Vec::new(),
        backup_path: Some(backup),
    })
}

fn recover(state: &AppState, config: &AppConfig) -> Result<(), String> {
    let path = dir(state).join("pending-toggle.json");
    let Some(text) = read_text(&path)? else {
        return Ok(());
    };
    let pending: Pending =
        serde_json::from_str(&text).map_err(|_| "插件待处理记录损坏，未自动恢复")?;
    let mut allowed = Vec::new();
    for agent in ["pi", "opencode"] {
        allowed.extend(inventory::config_paths(
            &inventory::root(config, agent)?,
            agent,
        ));
    }
    if !allowed.contains(&pending.path) {
        return Err("插件待处理配置路径不属于当前目录设置，未恢复".into());
    }
    let current = digest(read_text(&pending.path)?.unwrap_or_default().as_bytes());
    let catalog_path = dir(state).join("catalog.json");
    let catalog_hash = digest(read_text(&catalog_path)?.unwrap_or_default().as_bytes());
    let after_catalog_hash = digest(json_text(&pending.catalog_after)?.as_bytes());
    if catalog_hash != pending.catalog_before_hash && catalog_hash != after_catalog_hash {
        return Err("插件管理记录已被外部修改，未自动恢复".into());
    }
    if current == pending.after_hash {
        save_catalog(state, &pending.catalog_after)?;
    } else if current != pending.before_hash {
        return Err("原生配置已被外部修改，未完成上次停用记录；请核对快照，不会覆盖".into());
    }
    fs::remove_file(path).map_err(|_| "无法清理插件待处理记录")?;
    Ok(())
}

fn json_text(value: &impl Serialize) -> Result<String, String> {
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(value).map_err(|_| "插件记录无法序列化")?
    );
    if text.len() > 4 * 1024 * 1024 {
        return Err("插件管理记录超过 4 MiB，未写入".into());
    }
    Ok(text)
}
fn private_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    atomic::atomic_write_private(path, json_text(value)?.as_bytes())
        .map_err(|_| "插件管理记录写入失败".into())
}
fn save_catalog(state: &AppState, catalog: &Catalog) -> Result<(), String> {
    private_json(&dir(state).join("catalog.json"), catalog)
}

fn native_files(root: &Path, agent: &str) -> Vec<PathBuf> {
    if agent == "claude-code" {
        vec![
            root.join("settings.json"),
            root.join("plugins/installed_plugins.json"),
            root.join("plugins/known_marketplaces.json"),
        ]
    } else if agent == "pi" {
        vec![root.join("settings.json")]
    } else {
        inventory::config_paths(root, agent)
    }
}
fn native_signature(paths: &[PathBuf]) -> Result<String, String> {
    let mut data = Vec::new();
    for path in paths {
        data.extend_from_slice(read_text(path)?.unwrap_or_default().as_bytes());
        data.push(0);
    }
    Ok(digest(&data))
}
fn backup(state: &AppState, paths: &[PathBuf]) -> Result<PathBuf, String> {
    let folder = dir(state)
        .join("backups")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&folder).map_err(|_| "无法创建原生配置快照，未执行插件操作")?;
    let mut index = Vec::new();
    for (position, path) in paths.iter().enumerate() {
        if let Some(text) = read_text(path)? {
            let file = folder.join(format!("{position}.before"));
            atomic::atomic_write_private(&file, text.as_bytes())
                .map_err(|_| "无法保存原生配置快照，未执行插件操作")?;
            index.push(
                serde_json::json!({"path":path,"snapshot":file,"hash":digest(text.as_bytes())}),
            );
        } else {
            index.push(serde_json::json!({"path":path,"snapshot":null}));
        }
    }
    private_json(&folder.join("index.json"), &index)?;
    Ok(folder)
}
fn validate_source(source: &str) -> Result<(), String> {
    if source.trim().is_empty()
        || source.trim_start().starts_with('-')
        || source.chars().any(char::is_control)
    {
        return Err("插件来源为空、包含控制字符或以选项符号开头".into());
    }
    #[cfg(windows)]
    if source.contains(['&', '|', '<', '>', '^', '%']) {
        return Err("Windows 插件来源包含命令解释器字符，未执行".into());
    }
    let raw = source.strip_prefix("git:").unwrap_or(source);
    if let Ok(url) = url::Url::parse(raw) {
        if !url.username().is_empty() && !(url.scheme() == "ssh" && url.username() == "git")
            || url.password().is_some()
            || url.query().is_some()
        {
            return Err("插件来源不能包含凭据或查询参数；请使用原生 Git/npm 认证配置".into());
        }
    }
    Ok(())
}

fn same_source(agent: &str, first: &str, second: &str, root: &Path) -> bool {
    if agent == "claude-code" {
        first == second
    } else {
        source_identity(agent, first, root) == source_identity(agent, second, root)
    }
}

fn install_local_opencode(
    state: &AppState,
    root: &Path,
    source: &str,
    catalog: Catalog,
) -> Result<PluginOperationResult, String> {
    let path = inventory::config_paths(root, "opencode").remove(0);
    let before = read_text(&path)?.unwrap_or_default();
    let document =
        CstRootNode::parse(&before, &Default::default()).map_err(|_| "OpenCode 配置格式无效")?;
    let object = document
        .object_value_or_create()
        .ok_or("OpenCode 配置必须为对象")?;
    object
        .array_value_or_create("plugins")
        .ok_or("plugins 必须为数组")?
        .append(CstInputValue::String(source.into()));
    commit(state, &path, &before, &document.to_string(), &catalog)
}

fn in_auto_directory(path: &Path, auto: &Path) -> bool {
    path.canonicalize()
        .ok()
        .zip(auto.canonicalize().ok())
        .is_some_and(|(path, auto)| path.starts_with(auto))
}

fn command(agent: &str, action: PluginAction, source: &str) -> Result<Vec<String>, String> {
    match agent {
        "claude-code" => {
            let action = match action {
                PluginAction::Install => "install",
                PluginAction::Enable => "enable",
                PluginAction::Disable => "disable",
                PluginAction::Update => "update",
                PluginAction::Uninstall => "uninstall",
                _ => return Err("不支持该命令".into()),
            };
            let mut args = vec![
                "plugin".into(),
                action.into(),
                source.into(),
                "--scope".into(),
                "user".into(),
                "--json".into(),
            ];
            if action == "uninstall" {
                args.push("--keep-data".into());
            }
            Ok(args)
        }
        "pi" => {
            if action == PluginAction::Update && matches!(source, "self" | "pi") {
                return Err("插件更新不能触发 Pi 自身升级".into());
            }
            let action = match action {
                PluginAction::Install => "install",
                PluginAction::Update => "update",
                PluginAction::Uninstall => "remove",
                _ => return Err("Pi 启停使用资源过滤，不移除包".into()),
            };
            Ok(vec![action.into(), source.into(), "--no-approve".into()])
        }
        "opencode" => {
            let action = match action {
                PluginAction::Install => "add",
                PluginAction::Update => "update",
                PluginAction::Uninstall => "remove",
                _ => return Err("OpenCode 启停仅修改精确的配置包声明".into()),
            };
            Ok(vec!["plugin".into(), action.into(), source.into()])
        }
        _ => Err("不支持该 Agent 的原生插件写入".into()),
    }
}

#[derive(Debug)]
struct NativeOutput {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
}
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split(|character: char| !character.is_ascii_digit() && character != '.')
        .find_map(|word| {
            let parts: Vec<_> = word.split('.').collect();
            if parts.len() != 3 {
                return None;
            }
            Some((
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            ))
        })
}
fn already_in_goal_state(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout)
        .lines()
        .last()
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
        .is_some_and(|value| {
            value["alreadyInGoalState"].as_bool() == Some(true)
                && value["failureCode"] == "already_in_goal_state"
        })
}

fn run(
    program: &Path,
    args: &[String],
    root: &Path,
    agent: &str,
    timeout: Duration,
    capture_signals: bool,
) -> Result<NativeOutput, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "无法创建原生命令执行环境")?;
    runtime.block_on(async {
        let mut command = tokio::process::Command::new(program);
        command.args(args).current_dir(std::env::temp_dir()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        match agent { "claude-code" => { command.env("CLAUDE_CONFIG_DIR", root); }, "pi" => { command.env("PI_CODING_AGENT_DIR", root); }, "opencode" => { command.env("OPENCODE_CONFIG_DIR", root); command.env_remove("OPENCODE_CONFIG"); }, _ => {} }
        let mut command = CommandWrap::from(command); command.wrap(KillOnDrop);
        #[cfg(unix)] command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)] command.wrap(process_wrap::tokio::JobObject);
        let mut child = command.spawn().map_err(|_| "原生 CLI 无法启动")?;
        let stdout = child.stdout().take().ok_or("无法读取原生 CLI 输出")?;
        let stderr = child.stderr().take().ok_or("无法读取原生 CLI 错误输出")?;
        let mut stdout_task = tokio::spawn(drain(stdout)); let mut stderr_task = tokio::spawn(drain(stderr));
        let waited = tokio::select! {
            waited = tokio::time::timeout(timeout, child.wait()) => waited.map_err(|_| "原生插件命令超时").and_then(|status| status.map_err(|_| "原生插件命令等待失败")),
            _ = cancel_signal(capture_signals) => Err("原生插件操作已取消"),
        };
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        let stdout = tokio::time::timeout(Duration::from_secs(2), &mut stdout_task).await.ok().and_then(Result::ok).and_then(Result::ok).unwrap_or_default();
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut stderr_task).await;
        stdout_task.abort(); stderr_task.abort();
        let status = waited.map_err(str::to_owned)?;
        Ok(NativeOutput { success: status.success(), code: status.code(), stdout })
    })
}
async fn drain(mut pipe: impl AsyncRead + Unpin) -> Result<Vec<u8>, std::io::Error> {
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = pipe.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..count]);
        if output.len() > OUTPUT_LIMIT {
            output.drain(..output.len() - OUTPUT_LIMIT);
        }
    }
    Ok(output)
}
async fn cancel_signal(enabled: bool) {
    if !enabled {
        std::future::pending::<()>().await;
        return;
    }
    #[cfg(unix)]
    {
        let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            std::future::pending::<()>().await;
            return;
        };
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skill_studio_core::{
        models::{agent::AGENTS, config::AppConfig},
        services::store::Store,
    };

    #[test]
    fn native_commands_are_scoped_and_never_accept_blanket_trust_or_self_updates() {
        let args = command("claude-code", PluginAction::Uninstall, "example@market").unwrap();
        assert!(args.windows(2).any(|pair| pair == ["--scope", "user"]));
        assert!(args.contains(&"--keep-data".to_string()));
        assert!(!args.contains(&"--yes".to_string()));
        assert_eq!(
            command("pi", PluginAction::Update, "npm:pi-tools").unwrap(),
            ["update", "npm:pi-tools", "--no-approve"]
        );
        assert!(command("pi", PluginAction::Update, "pi").is_err());
        assert!(command("pi", PluginAction::Update, "self").is_err());
        assert!(command("grok", PluginAction::Install, "source").is_err());
    }

    fn state(temp: &Path) -> (AppState, AppConfig) {
        let mut config = AppConfig::default();
        config.settings.manage_plugins = true;
        config.settings.disabled_agents = AGENTS.iter().map(|agent| agent.id.into()).collect();
        config.settings.agent_dir_overrides = AGENTS
            .iter()
            .map(|agent| (agent.id.into(), temp.join(agent.id)))
            .collect();
        let store = Store::new(temp.join("studio"));
        atomic::write_json_file(&store.config_path(), &config).unwrap();
        (AppState::bootstrap(store).unwrap(), config)
    }

    #[test]
    fn interrupted_toggle_finishes_only_its_catalog_and_never_rewrites_native_config() {
        let temp = tempfile::tempdir().unwrap();
        let (state, config) = state(temp.path());
        let path = inventory::root(&config, "pi")
            .unwrap()
            .join("settings.json");
        let before = r#"{"packages":["npm:example"]}"#;
        let after = r#"{"packages":[{"source":"npm:example","skills":[],"extensions":[],"prompts":[],"themes":[]}]}"#;
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, after).unwrap();
        let catalog = Catalog {
            paused: vec![PausedPlugin {
                id: key("pi", &path, "npm:example"),
                agent: "pi".into(),
                config_path: path.clone(),
                source: "npm:example".into(),
                original: Value::String("npm:example".into()),
                applied: serde_json::from_str::<Value>(after).unwrap()["packages"][0].clone(),
                index: 0,
                package_root: None,
                field: None,
            }],
        };
        private_json(
            &dir(&state).join("pending-toggle.json"),
            &Pending {
                path: path.clone(),
                before_hash: digest(before.as_bytes()),
                after_hash: digest(after.as_bytes()),
                catalog_before_hash: digest(b""),
                catalog_after: catalog,
            },
        )
        .unwrap();
        let result = repair(&state, true).unwrap();
        assert!(result.changed);
        assert_eq!(fs::read_to_string(path).unwrap(), after);
        assert_eq!(read_catalog(&state).unwrap().paused.len(), 1);
        assert!(!dir(&state).join("pending-toggle.json").exists());
    }

    #[test]
    fn pending_recovery_preserves_foreign_changes_and_leaves_diagnostic_marker() {
        let temp = tempfile::tempdir().unwrap();
        let (state, config) = state(temp.path());
        let path = inventory::root(&config, "pi")
            .unwrap()
            .join("settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{\"outside\":true}").unwrap();
        private_json(
            &dir(&state).join("pending-toggle.json"),
            &Pending {
                path: path.clone(),
                before_hash: digest(b"before"),
                after_hash: digest(b"after"),
                catalog_before_hash: digest(b""),
                catalog_after: Catalog::default(),
            },
        )
        .unwrap();
        assert!(repair(&state, true).unwrap_err().contains("外部修改"));
        assert_eq!(fs::read_to_string(path).unwrap(), "{\"outside\":true}");
        assert!(dir(&state).join("pending-toggle.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn native_output_is_bounded_and_timeout_kills_the_owned_process_group() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let program = temp.path().join("native");
        fs::write(
            &program,
            "#!/bin/sh\nhead -c 100000 /dev/zero | tr '\\000' x\necho END\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let output = run(
            &program,
            &[],
            temp.path(),
            "pi",
            Duration::from_secs(5),
            false,
        )
        .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout.len(), OUTPUT_LIMIT);
        assert!(output.stdout.ends_with(b"END\n"));
        fs::write(&program, "#!/bin/sh\nsleep 30 &\nwait\n").unwrap();
        let started = std::time::Instant::now();
        assert!(run(
            &program,
            &[],
            temp.path(),
            "pi",
            Duration::from_millis(50),
            false
        )
        .unwrap_err()
        .contains("超时"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
