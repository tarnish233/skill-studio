use super::*;
use skill_studio_core::{
    models::agent::{require_agent, AGENTS},
    services::plugins,
};

pub(super) fn scan(config: &AppConfig, catalog: &Catalog) -> (Vec<Found>, Vec<String>) {
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    for agent in AGENTS {
        if config
            .settings
            .disabled_agents
            .iter()
            .any(|id| id == agent.id)
        {
            continue;
        }
        let root = agent.resolved_config_dir(&config.settings.agent_dir_overrides);
        let result = match agent.id {
            "claude-code" => claude(&root, &mut found, &mut warnings),
            "pi" => packages(&root, "pi", catalog, &mut found, &mut warnings),
            "opencode" => packages(&root, "opencode", catalog, &mut found, &mut warnings),
            "codex" => codex(&root, &mut found, &mut warnings),
            "grok" => grok(&root, &mut found, &mut warnings),
            "deepseek-harness" => dsh(&root, &mut found, &mut warnings),
            _ => Ok(()),
        };
        if let Err(error) = result {
            warnings.push(format!("{}：{error}", agent.display_name));
        }
    }
    found.sort_by(|a, b| {
        a.view
            .package
            .name
            .to_lowercase()
            .cmp(&b.view.package.name.to_lowercase())
            .then(a.view.id.cmp(&b.view.id))
    });
    found.dedup_by(|a, b| a.view.id == b.view.id);
    (found, warnings)
}

fn view(
    agent: &str,
    config: &Path,
    source: &str,
    declaration: &Value,
    package: PluginPackage,
) -> PluginView {
    let native_id = display_source(source);
    let revision = digest(
        serde_json::to_string(&(declaration, &package))
            .unwrap()
            .as_bytes(),
    );
    PluginView {
        id: key(agent, config, source),
        agent_id: agent.into(),
        native_id: native_id.clone(),
        source: native_id,
        scope: "user".into(),
        profile: None,
        config_path: config.into(),
        available: !package.root.as_os_str().is_empty() && package.root.exists(),
        configured: true,
        enabled: None,
        revision,
        actions: Vec::new(),
        read_only_reason: None,
        package,
    }
}

fn unknown(source: &str, format: &str) -> PluginPackage {
    PluginPackage {
        name: npm_name(source)
            .map(str::to_owned)
            .unwrap_or_else(|| display_source(source)),
        format: format.into(),
        error: Some("未找到可读包目录；仅展示原生配置声明".into()),
        ..Default::default()
    }
}

fn marketplace_entry(root: &Path, id: &str) -> Option<Value> {
    let (name, marketplace) = id.rsplit_once('@')?;
    let known: Value =
        serde_json::from_str(&read_text(&root.join("plugins/known_marketplaces.json")).ok()??)
            .ok()?;
    let location = PathBuf::from(known.get(marketplace)?.get("installLocation")?.as_str()?);
    for relative in [".claude-plugin/marketplace.json", "marketplace.json"] {
        let path = location.join(relative);
        let Some(text) = read_text(&path).ok().flatten() else {
            continue;
        };
        let document: Value = serde_json::from_str(&text).ok()?;
        let entry = document["plugins"]
            .as_array()?
            .iter()
            .find(|entry| entry["name"].as_str() == Some(name))?;
        return Some(entry.clone());
    }
    None
}

fn claude(root: &Path, found: &mut Vec<Found>, warnings: &mut Vec<String>) -> Result<(), String> {
    let config_path = root.join("settings.json");
    let settings = config_document(&config_path, "claude-code")?;
    let installed_path = root.join("plugins/installed_plugins.json");
    if let Some(text) = read_text(&installed_path)? {
        let installed: Value = serde_json::from_str(&text)
            .map_err(|_| "installed_plugins.json 格式无效；未扫描旧缓存代替安装记录")?;
        let records = installed["plugins"]
            .as_object()
            .ok_or("插件安装记录缺少 plugins 对象")?;
        for (id, records) in records {
            let records: Vec<&Value> = records
                .as_array()
                .map(|records| records.iter().collect())
                .unwrap_or_else(|| vec![records]);
            for record in records {
                let scope = record["scope"].as_str().unwrap_or("unknown");
                let path = record["installPath"].as_str().map(PathBuf::from);
                let entry = marketplace_entry(root, id);
                let mut package = path
                    .as_ref()
                    .map(|path| {
                        plugins::inspect_with_manifest(path, Some("claude-code"), entry.as_ref())
                    })
                    .unwrap_or_else(|| unknown(id, "claude"));
                package.name = id.split('@').next().unwrap_or(id).to_string();
                if package.version.is_none() {
                    let version = record["version"].as_str();
                    let commit = record["gitCommitSha"].as_str();
                    if !version
                        .zip(commit)
                        .is_some_and(|(version, commit)| commit.starts_with(version))
                        && !version.is_some_and(|version| version.eq_ignore_ascii_case("unknown"))
                    {
                        package.version = version.map(str::to_owned);
                    }
                }
                let state =
                    serde_json::json!({"record":record,"enabled":settings["enabledPlugins"][id]});
                let mut item = view("claude-code", &config_path, id, &state, package);
                item.scope = scope.into();
                if scope == "user" {
                    // Claude 未在 enabledPlugins 中登记的插件默认启用。
                    item.enabled = settings["enabledPlugins"][id].as_bool().or(Some(true));
                    item.actions = vec![
                        PluginAction::Enable,
                        PluginAction::Disable,
                        PluginAction::Update,
                        PluginAction::Uninstall,
                    ];
                } else {
                    item.id = key(
                        "claude-code",
                        &installed_path,
                        &format!("{id}\0{scope}\0{}", record["projectPath"]),
                    );
                    item.read_only_reason =
                        Some("首版仅管理个人 user 作用域；项目、本地和组织安装只读".into());
                }
                found.push(Found {
                    view: item,
                    source: id.clone(),
                    declaration: state,
                    index: None,
                    paused: None,
                    field: None,
                });
            }
        }
    }
    let skill_root = root.join("skills");
    for path in children(&skill_root)? {
        if !plugins::is_plugin_directory(&path) {
            continue;
        }
        let package = plugins::inspect(&path, Some("claude-code"));
        let id = format!("{}@skills-dir", package.name);
        let state = serde_json::json!({"path":path,"enabled":settings["enabledPlugins"][&id]});
        let mut item = view("claude-code", &config_path, &id, &state, package);
        // 同上：skills-dir 插件未登记时默认启用。
        item.enabled = settings["enabledPlugins"][&id].as_bool().or(Some(true));
        item.actions = vec![PluginAction::Enable, PluginAction::Disable];
        item.package
            .warnings
            .push("本地 skills-dir 插件不生成市场、不迁移原件；首版仅管理个人启停".into());
        found.push(Found {
            view: item,
            source: id,
            declaration: state,
            index: None,
            paused: None,
            field: None,
        });
    }
    if settings
        .get("enabledPlugins")
        .is_some_and(|value| !value.is_object())
    {
        warnings.push("Claude enabledPlugins 不是对象，未推断启用状态".into());
    }
    Ok(())
}

pub(super) fn config_paths(root: &Path, agent: &str) -> Vec<PathBuf> {
    if agent == "pi" {
        vec![root.join("settings.json")]
    } else {
        let paths: Vec<_> = ["opencode.jsonc", "opencode.json"]
            .into_iter()
            .map(|name| root.join(name))
            .filter(|path| path.exists())
            .collect();
        if paths.is_empty() {
            vec![root.join("opencode.jsonc")]
        } else {
            paths
        }
    }
}

fn packages(
    root: &Path,
    agent: &str,
    catalog: &Catalog,
    found: &mut Vec<Found>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let paths = config_paths(root, agent);
    for config_path in &paths {
        let doc = config_document(config_path, agent)?;
        // OpenCode 新旧两个插件键并存时都扫描；每个声明记住自己所在键，编辑时精确定位。
        let fields: Vec<&'static str> = if agent == "opencode" {
            ["plugins", "plugin"]
                .into_iter()
                .filter(|field| !doc.get(*field).is_none_or(Value::is_null))
                .collect()
        } else {
            vec![if agent == "pi" { "packages" } else { "plugins" }]
        };
        let mut entries: Vec<(&'static str, usize, Value)> = Vec::new();
        for field in fields {
            let Some(value) = doc.get(field) else {
                continue;
            };
            let array = value
                .as_array()
                .ok_or_else(|| format!("{field} 必须为数组；未按空列表处理"))?;
            for (index, declaration) in array.iter().enumerate() {
                entries.push((field, index, declaration.clone()));
            }
        }
        let identities: Vec<String> = entries
            .iter()
            .filter_map(|(_, _, declaration)| source_of(declaration, agent))
            .map(|source| source_identity(agent, source, root))
            .collect();
        let mut seen = Vec::new();
        for (field, index, declaration) in entries {
            let Some(source) = source_of(&declaration, agent) else {
                warnings.push(format!("{agent} 忽略缺少来源的插件声明"));
                continue;
            };
            if agent == "opencode"
                && (source == "*" || source.starts_with('-') || source.ends_with(".*"))
            {
                continue;
            }
            let id = key(agent, config_path, source);
            let paused = catalog
                .paused
                .iter()
                .find(|record| record.id == id)
                .cloned();
            let path = package_root(root, agent, source);
            let package = path
                .as_ref()
                .map(|path| plugins::inspect(path, Some(agent)))
                .unwrap_or_else(|| {
                    unknown(
                        source,
                        if agent == "pi" {
                            "pi-package"
                        } else {
                            "opencode"
                        },
                    )
                });
            let mut item = view(agent, config_path, source, &declaration, package);
            item.enabled = Some(if agent == "pi" {
                !["extensions", "skills", "prompts", "themes"]
                    .iter()
                    .all(|field| {
                        declaration
                            .get(*field)
                            .and_then(Value::as_array)
                            .is_some_and(Vec::is_empty)
                    })
            } else {
                true
            });
            item.actions = vec![
                PluginAction::Enable,
                PluginAction::Disable,
                PluginAction::Update,
                PluginAction::Uninstall,
            ];
            if package_local_path(agent, source, root).is_some() {
                item.actions
                    .retain(|action| *action != PluginAction::Update);
            }
            if paused
                .as_ref()
                .is_some_and(|record| declaration != record.applied)
            {
                item.read_only_reason =
                    Some("停用后配置已被外部修改；仅可移除 Studio 停用记录，不覆盖原生配置".into());
                item.actions = vec![PluginAction::Forget];
            }
            if declaration["autoload"].as_bool() == Some(false) {
                item.read_only_reason =
                    Some("autoload 资源增量配置由 Pi 管理，首版不改变其语义".into());
                item.actions.clear();
            }
            if agent == "pi" && item.package.format == "extension" {
                item.enabled = None;
                item.actions
                    .retain(|action| *action == PluginAction::Uninstall);
                item.read_only_reason = Some(
                    "单文件包的资源过滤尚未验证，仅可通过原生工具移除声明，不删除原文件".into(),
                );
            }
            let identity = source_identity(agent, source, root);
            let duplicates = identities
                .iter()
                .filter(|other| **other == identity)
                .count();
            if duplicates > 1 {
                item.read_only_reason = Some("同一原生包有多个声明，未猜测应修改哪一项".into());
                item.actions.clear();
            }
            if paths.len() > 1 {
                item.read_only_reason = Some("存在多个全局配置文件，首版只读以避免误改".into());
                item.actions.clear();
            }
            if agent == "opencode" && path.is_none() && declaration.is_string() {
                item.read_only_reason =
                    Some("无法确定这是包声明还是原生插件 ID 规则，未猜测或执行插件代码".into());
                item.actions.clear();
            }
            if agent == "opencode"
                && path
                    .as_ref()
                    .is_some_and(|path| in_auto_directory(path, &root.join("plugins")))
            {
                item.read_only_reason =
                    Some("该目录还会被 OpenCode 自动发现；撤回声明不能保证停用，首版只读".into());
                item.actions.clear();
            }
            if agent == "opencode" {
                item.package.warnings.push("启停仅撤回/还原这个个人配置的包声明，保留原生缓存，不猜代码里的插件 ID；项目声明不受影响".into());
            }
            seen.push(id);
            found.push(Found {
                view: item,
                source: source.into(),
                declaration,
                index: Some(index),
                paused,
                field: Some(field.to_string()),
            });
        }
        for paused in catalog.paused.iter().filter(|record| {
            record.agent == agent
                && record.config_path == *config_path
                && !seen.contains(&record.id)
        }) {
            let package = paused
                .package_root
                .as_ref()
                .map(|path| plugins::inspect(path, Some(agent)))
                .unwrap_or_else(|| {
                    unknown(
                        &paused.source,
                        if agent == "pi" {
                            "pi-package"
                        } else {
                            "opencode"
                        },
                    )
                });
            let mut item = view(agent, config_path, &paused.source, &Value::Null, package);
            item.configured = false;
            item.enabled = Some(false);
            item.actions = vec![PluginAction::Enable, PluginAction::Forget];
            if agent == "pi" {
                item.read_only_reason =
                    Some("Pi 的原生声明已被移除；仅可移除停用记录，不重新创建声明".into());
                item.actions = vec![PluginAction::Forget];
            }
            let identity = source_identity(agent, &paused.source, root);
            if identities.contains(&identity) {
                item.read_only_reason =
                    Some("原生配置已有同一包的其他版本或路径，未恢复旧声明".into());
                item.actions = vec![PluginAction::Forget];
            }
            found.push(Found {
                view: item,
                source: paused.source.clone(),
                declaration: Value::Null,
                index: None,
                paused: Some(paused.clone()),
                field: paused.field.clone(),
            });
        }
    }
    for path in children(&root.join(if agent == "pi" {
        "extensions"
    } else {
        "plugins"
    }))? {
        if !path.is_dir()
            && !matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("ts" | "js" | "mjs" | "cjs")
            )
        {
            continue;
        }
        if found.iter().any(|item| {
            item.view.agent_id == agent
                && item.view.package.root == path.canonicalize().unwrap_or_else(|_| path.clone())
        }) {
            continue;
        }
        let source = path.to_string_lossy();
        let package = plugins::inspect(&path, Some(agent));
        let state = serde_json::json!({"path":path});
        let mut item = view(agent, &paths[0], &source, &state, package);
        item.read_only_reason =
            Some("自动发现的扩展只读；首版管理原生配置中的包声明，不移动或删除扩展文件".into());
        found.push(Found {
            view: item,
            source: source.into_owned(),
            declaration: state,
            index: None,
            paused: None,
            field: None,
        });
    }
    Ok(())
}

fn in_auto_directory(path: &Path, auto: &Path) -> bool {
    path.canonicalize()
        .ok()
        .zip(auto.canonicalize().ok())
        .is_some_and(|(path, auto)| path.starts_with(auto))
}

pub(super) fn package_root(root: &Path, agent: &str, source: &str) -> Option<PathBuf> {
    if let Some(local) = package_local_path(agent, source, root) {
        return Some(local);
    }
    if let Some(name) = npm_name(source) {
        let path = root
            .join(if agent == "pi" {
                "npm/node_modules"
            } else {
                "node_modules"
            })
            .join(name);
        if path.is_dir() {
            return Some(path);
        }
        if agent == "opencode" {
            return opencode_npm_cache(name, source);
        }
        return None;
    }
    if agent == "opencode" {
        if let Ok(manifest) = config_document(&root.join("package.json"), "claude-code") {
            if let Some(dependencies) = manifest["dependencies"].as_object() {
                for (name, value) in dependencies {
                    if npm_name(name) == Some(name.as_str())
                        && value.as_str().is_some_and(|spec| spec == source)
                    {
                        let path = root.join("node_modules").join(name);
                        if path.exists() {
                            return Some(path);
                        }
                    }
                }
            }
        }
        return None;
    }
    let source = source.strip_prefix("git:").unwrap_or(source);
    let source = source
        .rfind('@')
        .filter(|index| *index > source.rfind('/').unwrap_or(usize::MAX))
        .map(|index| &source[..index])
        .unwrap_or(source);
    let source = if source.starts_with("git@") {
        format!("ssh://{}", source.replacen(':', "/", 1))
    } else if source.contains("://") {
        source.into()
    } else {
        format!("https://{source}")
    };
    let url = url::Url::parse(&source).ok()?;
    let host = url.host_str()?;
    let repo = url.path().trim_start_matches('/').trim_end_matches(".git");
    if host.is_empty()
        || repo.is_empty()
        || repo.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':'])
        })
    {
        return None;
    }
    let path = root.join("git").join(host).join(repo);
    path.exists().then_some(path)
}

/// OpenCode 2.3+ 把 npm 包装进用户缓存
/// `~/.cache/opencode/npm/<名称>@<规格>/<时间戳>/node_modules/<名称>/`，取最新时间戳。
fn opencode_npm_cache(name: &str, source: &str) -> Option<PathBuf> {
    let spec = source.strip_prefix("npm:").unwrap_or(source);
    let spec = spec
        .rfind('@')
        .filter(|at| *at > 0)
        .map(|at| &spec[at + 1..])
        .filter(|spec| !spec.is_empty())
        .unwrap_or("latest");
    let base = paths::home_dir().join(".cache/opencode/npm");
    for spec in [spec, "latest"] {
        let dir = base.join(format!("{name}@{spec}"));
        let mut newest: Option<(u64, PathBuf)> = None;
        for entry in fs::read_dir(&dir).ok()?.flatten() {
            let Ok(stamp) = entry.file_name().to_string_lossy().parse::<u64>() else {
                continue;
            };
            let package = entry.path().join("node_modules").join(name);
            if !package.is_dir() {
                continue;
            }
            if newest.as_ref().is_none_or(|(known, _)| stamp > *known) {
                newest = Some((stamp, entry.path()));
            }
        }
        if let Some((_, dir)) = newest {
            return Some(dir.join("node_modules").join(name));
        }
    }
    None
}

fn codex(root: &Path, found: &mut Vec<Found>, warnings: &mut Vec<String>) -> Result<(), String> {
    let config_path = root.join("config.toml");
    let settings = if config_path.exists() {
        config_document(&config_path, "codex")?
    } else {
        serde_json::json!({})
    };
    let Some(entries) = settings.get("plugins") else {
        return Ok(());
    };
    let entries = entries.as_object().ok_or("plugins 必须为对象")?;
    for (id, declaration) in entries {
        let candidate = id
            .rsplit_once('@')
            .filter(|(name, marketplace)| {
                safe_directory_name(name) && safe_directory_name(marketplace)
            })
            .and_then(|(name, marketplace)| {
                let versions =
                    children(&root.join("plugins/cache").join(marketplace).join(name)).ok()?;
                (versions.len() == 1).then(|| versions[0].clone())
            });
        let mut package = candidate
            .map(|path| plugins::inspect(&path, Some("codex")))
            .unwrap_or_else(|| unknown(id, "codex"));
        package.warnings.push(
            "版本和组件来自可唯一定位的本机缓存，不代表 Codex 当前会话已加载；多个缓存版本时不猜测"
                .into(),
        );
        let mut item = view("codex", &config_path, id, declaration, package);
        item.enabled = declaration["enabled"].as_bool();
        item.read_only_reason = Some("Codex 插件首版仅展示，原生写入不在本轮".into());
        found.push(Found {
            view: item,
            source: id.clone(),
            declaration: declaration.clone(),
            index: None,
            paused: None,
            field: None,
        });
    }
    if !root.join("plugins/cache").exists() && !entries.is_empty() {
        warnings.push("Codex 有插件声明，但没有本机插件缓存".into());
    }
    Ok(())
}

fn grok(root: &Path, found: &mut Vec<Found>, warnings: &mut Vec<String>) -> Result<(), String> {
    let config_path = root.join("config.toml");
    let settings = if config_path.exists() {
        config_document(&config_path, "grok")?
    } else {
        serde_json::json!({})
    };
    let mut roots = vec![root.join("plugins")];
    if let Some(extra) = settings["plugins"]["paths"].as_array() {
        roots.extend(
            extra
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|source| local_source(source, root)),
        );
    }
    for scan_root in roots {
        let candidates = if scan_root != root.join("plugins")
            && plugins::inspect(&scan_root, Some("grok")).error.is_none()
        {
            vec![scan_root]
        } else {
            children(&scan_root)?
        };
        for path in candidates {
            if !path.is_dir()
                || matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("marketplaces" | "cache" | "data")
                )
            {
                continue;
            }
            let package = plugins::inspect(&path, Some("grok"));
            if package.error.is_some() {
                continue;
            }
            let name = package.name.clone();
            let state = serde_json::json!({"path":path,"enabled":settings["plugins"]["enabled"],"disabled":settings["plugins"]["disabled"]});
            let mut item = view("grok", &config_path, &name, &state, package);
            let contains = |key: &str| {
                settings["plugins"][key].as_array().is_some_and(|entries| {
                    entries.iter().any(|entry| entry.as_str() == Some(&name))
                })
            };
            item.enabled = if contains("disabled") {
                Some(false)
            } else if contains("enabled") {
                Some(true)
            } else {
                None
            };
            item.id = key("grok", &config_path, &path.to_string_lossy());
            item.read_only_reason =
                Some("Grok 首版只读；启用、信任和 Claude 兼容读取必须分别处理".into());
            found.push(Found {
                view: item,
                source: name,
                declaration: state,
                index: None,
                paused: None,
                field: None,
            });
        }
    }
    if root.join("plugins/marketplaces").exists() {
        warnings.push("Grok 市场缓存和兼容读取不作为已安装/已信任插件推断".into());
    }
    Ok(())
}

fn dsh(root: &Path, found: &mut Vec<Found>, _warnings: &mut Vec<String>) -> Result<(), String> {
    // DSH 是只读展示：同一 Bundle 常被多个 Profile 选中，按 Bundle 聚合成一行，
    // 列出使用它的 Profile，避免同名卡片重复。
    let mut bundles: std::collections::BTreeMap<String, Vec<PathBuf>> =
        std::collections::BTreeMap::new();
    for profile in children(&root.join("profiles"))? {
        if !profile.is_dir() {
            continue;
        }
        let config_path = profile.join("package.json");
        if !config_path.exists() {
            continue;
        }
        let manifest = config_document(&config_path, "deepseek-harness")?;
        let Some(list) = manifest["dsh"]["profile"]["bundles"].as_array() else {
            continue;
        };
        for bundle in list.iter().filter_map(Value::as_str) {
            if npm_name(bundle) != Some(bundle) {
                continue;
            }
            bundles
                .entry(bundle.to_string())
                .or_default()
                .push(profile.clone());
        }
    }
    let profiles_root = root.join("profiles");
    for (bundle, profiles) in bundles {
        let names: Vec<String> = profiles
            .iter()
            .filter_map(|profile| {
                profile
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .collect();
        let package_root = profiles
            .iter()
            .map(|profile| profile.join("node_modules").join(&bundle))
            .find(|path| path.exists());
        let package = package_root
            .map(|path| plugins::inspect(&path, Some("deepseek-harness")))
            .unwrap_or_else(|| unknown(&bundle, "dsh-bundle"));
        let declaration = serde_json::json!({"bundle":bundle,"profiles":names});
        let mut item = view(
            "deepseek-harness",
            &profiles_root,
            &bundle,
            &declaration,
            package,
        );
        item.scope = "profile".into();
        item.enabled = Some(true);
        match names.len() {
            1 => item.profile = names.first().cloned(),
            0 => {}
            _ => item.package.warnings.push(format!(
                "由 {} 个 Profile 选中：{}",
                names.len(),
                names.join("、")
            )),
        }
        item.read_only_reason = Some("仅展示 Profile 选中的 Bundle，未挂载配置或运行代码；实际加载状态与停用覆盖由 Harness 决定".into());
        found.push(Found {
            view: item,
            source: bundle.clone(),
            declaration,
            index: None,
            paused: None,
            field: None,
        });
    }
    Ok(())
}

fn safe_directory_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}
fn children(path: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(format!("无法读取 {}", path.display())),
    };
    let mut result = Vec::new();
    for entry in entries {
        result.push(entry.map_err(|_| "插件目录条目不可读")?.path());
    }
    result.sort();
    Ok(result)
}

pub(super) fn resolve(config: &AppConfig, catalog: &Catalog, id: &str) -> Result<Found, String> {
    let (found, _) = scan(config, catalog);
    found
        .into_iter()
        .find(|found| found.view.id == id)
        .ok_or_else(|| "插件已被移除、目录已变更或来源不再可读；请刷新后重试".into())
}

pub(super) fn root(config: &AppConfig, agent: &str) -> Result<PathBuf, String> {
    Ok(require_agent(agent)
        .map_err(String::from)?
        .resolved_config_dir(&config.settings.agent_dir_overrides))
}
