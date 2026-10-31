//! 共享根 `~/.agents/skills` 被 Codex、OpenCode、Pi、Grok 同时读取：放进去的 skill
//! 每个 Agent 都会加载，分组和原生开关都管不住。Studio 从不代替某一个 Agent 往这里写——
//! 纳入托管的回链、移出 Hub、退出管理时的还原，都落在该 Agent 自己的目录。
#[path = "support.rs"]
mod support;
use serial_test::serial;
use skill_studio_core::models::agent::in_shared_root;
use skill_studio_core::models::skill::LinkStatus;
use skill_studio_core::services::studio::SkillView;
use std::{collections::HashMap, fs};
use support::Env;

/// Agent 启动时会加载它（来源、链接或副本都算）
fn loads(view: &SkillView, agent: &str) -> bool {
    use LinkStatus::*;
    matches!(
        view.agents[agent].status,
        Source | Linked | Copied | CopyStale | CopyModified
    )
}

fn shared_root_is_empty(env: &Env) -> bool {
    fs::read_dir(env.agents_skills())
        .map(|entries| entries.count() == 0)
        .unwrap_or(true)
}

fn setup() -> Env {
    let env = Env::new();
    // 让 Pi 被识别为已安装：它和 Codex 一样会读共享根
    fs::create_dir_all(env.path().join(".pi/agent")).unwrap();
    env
}

#[test]
#[serial]
fn adopting_a_shared_root_skill_moves_it_out_and_keeps_only_its_owner() {
    let env = setup();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.agents_skills(), "yuque");
    let view = studio.scan_skills(&c).unwrap().remove(0);
    assert!(
        loads(&view, "pi"),
        "前提：共享根里的 skill 本来就会被 Pi 加载"
    );

    let hub = studio.adopt_to_hub(&mut c, &view.skill.id).unwrap();

    assert!(shared_root_is_empty(&env), "共享根里不能留回链");
    let codex_entry = env.codex_skills().join("yuque");
    assert_eq!(
        c.registrations[&hub.id]["codex"].target_path, codex_entry,
        "所属 Agent 的入口改到它自己的目录"
    );
    let view = studio.scan_skills(&c).unwrap().remove(0);
    assert_eq!(view.agents["codex"].status, LinkStatus::Linked);
    for agent in ["pi", "opencode", "grok", "claude-code"] {
        assert!(!loads(&view, agent), "{agent} 不应再读到它");
    }
}

/// 用户机器上的完整链路：原件在共享根、Codex 目录里是指向它的软链 → 纳入托管 →
/// 关闭 Codex 管理 → 重新开启。旧实现会在关闭管理时把原件还原进共享根，Pi 从此在
/// 没有启用任何分组的情况下加载它。
#[test]
#[serial]
#[cfg(unix)]
fn leaving_and_reenabling_codex_never_writes_the_shared_root() {
    let env = setup();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let original = env.write_simple_skill(&env.agents_skills(), "yuque");
    fs::create_dir_all(env.codex_skills()).unwrap();
    let alias = env.codex_skills().join("yuque");
    std::os::unix::fs::symlink(&original, &alias).unwrap();
    let source = studio.scan_skills(&c).unwrap().remove(0).skill;
    studio.adopt_to_hub(&mut c, &source.id).unwrap();
    c.settings.preserve_manual_skills = false;
    assert!(shared_root_is_empty(&env));

    studio.set_agent_management(&mut c, "codex", false).unwrap();
    assert!(shared_root_is_empty(&env), "关闭管理时不能还原进共享根");
    assert!(
        alias.join("SKILL.md").is_file(),
        "Codex 拿回自己目录里的原件"
    );
    assert!(!fs::symlink_metadata(&alias)
        .unwrap()
        .file_type()
        .is_symlink());

    studio.set_agent_management(&mut c, "codex", true).unwrap();
    assert!(shared_root_is_empty(&env));
    assert!(c.active_groups.is_empty());
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views.len(), 1, "还原件和 Hub 里的仍是同一个 skill");
    assert!(!loads(&views[0], "pi"), "Pi 没启用任何分组，不应加载它");
    // 不保留组外 skill：Codex 用原生开关停用，文件留着
    assert!(views[0].agents["codex"].disabled);
}

/// 旧版本留下的状态：回链放在共享根并登记给 Codex，来源记录里也只有共享根的入口。
/// 关闭 Codex 管理要把共享根里的回链撤掉，并把原件还原到 Codex 自己的目录。
#[test]
#[serial]
#[cfg(unix)]
fn leaving_codex_cleans_a_legacy_shared_link_back_and_restores_into_its_own_root() {
    let env = setup();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let shared = env.write_simple_skill(&env.agents_skills(), "yuque");
    let source = studio.scan_skills(&c).unwrap().remove(0).skill;
    let hub = studio.adopt_to_hub(&mut c, &source.id).unwrap();
    // 伪造旧版本纳入托管后的样子
    let codex_entry = env.codex_skills().join("yuque");
    fs::remove_file(&codex_entry).unwrap();
    std::os::unix::fs::symlink(&hub.source_path, &shared).unwrap();
    c.registrations
        .get_mut(&hub.id)
        .unwrap()
        .get_mut("codex")
        .unwrap()
        .target_path = shared.clone();
    c.skill_provenance.get_mut(&hub.id).unwrap().entry_paths = vec![shared.clone()];
    studio.save_config(&c).unwrap();
    assert!(loads(&studio.scan_skills(&c).unwrap()[0], "pi"));

    studio.set_agent_management(&mut c, "codex", false).unwrap();

    assert!(shared_root_is_empty(&env), "共享根里的旧回链要撤掉");
    assert!(codex_entry.join("SKILL.md").is_file());
    assert!(!fs::symlink_metadata(&codex_entry)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(c.skill_provenance[&hub.id]
        .entry_paths
        .contains(&codex_entry));
    assert!(c.registrations.is_empty());
    let views = studio.scan_skills(&c).unwrap();
    assert_eq!(views.len(), 1, "新入口要算作同一个 skill，不能变成两条");
    assert!(!loads(&views[0], "pi"));
    assert!(loads(&views[0], "codex"));
}

#[test]
#[serial]
fn shared_root_ownership_is_direct_child_only() {
    let env = Env::new();
    std::env::set_var("DSH_AGENTS_HOME", env.path().join("custom-shared"));
    let root = env.path().join("custom-shared/skills");
    let overrides = HashMap::new();
    assert!(!in_shared_root(&root, &overrides));
    assert!(in_shared_root(&root.join("demo"), &overrides));
    assert!(!in_shared_root(&root.join("nested/demo"), &overrides));
    std::env::remove_var("DSH_AGENTS_HOME");
}
