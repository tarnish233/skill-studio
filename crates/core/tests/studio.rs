//! Studio 编排层的端到端行为。

#[path = "support.rs"]
mod support;

use serial_test::serial;
use skill_studio_core::models::group::Group;
use skill_studio_core::models::project::ProjectBinding;
use skill_studio_core::models::skill::{LinkMode, LinkStatus, SkillOrigin};
use support::Env;

fn state(view: &skill_studio_core::services::studio::SkillView, agent: &str) -> LinkStatus {
    view.agents.get(agent).unwrap().status
}

#[test]
#[serial]
fn scans_in_place_skills_and_marks_owner_as_source() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "pdf-tools");
    let studio = env.studio();
    let config = studio.load_config().unwrap();

    let views = studio.scan_skills(&config).unwrap();
    assert_eq!(views.len(), 1);
    let v = &views[0];
    assert_eq!(v.skill.name, "pdf-tools");
    assert_eq!(v.skill.display_name.as_deref(), Some("pdf-tools"));
    assert_eq!(v.skill.description.as_deref(), Some("测试用 pdf-tools"));
    assert!(matches!(
        v.skill.origin,
        SkillOrigin::InPlace { ref owner_agent } if owner_agent == "claude-code"
    ));
    // 真身所在的 agent 是 Source，另一个还没有
    assert_eq!(state(v, "claude-code"), LinkStatus::Source);
    assert_eq!(state(v, "codex"), LinkStatus::NotLinked);
}

#[test]
#[serial]
fn registers_across_agents_and_reports_linked() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "pdf-tools");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    let report = studio
        .register(
            &mut config,
            std::slice::from_ref(&id),
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();
    assert!(report.is_all_ok(), "{report:?}");
    assert_eq!(report.success.len(), 1);

    // 目标真的落在 Codex 的写入根上
    let dest = env.codex_skills().join("pdf-tools");
    assert!(dest.join("SKILL.md").is_file());

    let views = studio.scan_skills(&config).unwrap();
    let v = views.iter().find(|v| v.skill.id == id).unwrap();
    assert_eq!(state(v, "codex"), LinkStatus::Linked);
    assert_eq!(state(v, "claude-code"), LinkStatus::Source);
    // 注册记录已落到配置里
    assert!(config.registration(&id, "codex").is_some());
}

#[test]
#[serial]
fn detects_skill_placed_in_codex_shared_root() {
    // ~/.agents/skills 与 ~/.codex/skills 同为 Codex 的 User scope 根，
    // 只看写入根会把这里的 skill 误报成未注册
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "shared");
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    // 手工把它复制到共享根
    let shared = env.agents_skills().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("SKILL.md"), "---\nname: shared\n---\n").unwrap();

    let views = studio.scan_skills(&config).unwrap();
    let v = views.iter().find(|v| v.skill.id == id).unwrap();
    // 共享根里那份没有边车，是用户自己放的 → Foreign，需要用户关注
    assert_eq!(state(v, "codex"), LinkStatus::Foreign);
    assert!(state(v, "codex").needs_attention());
}

#[test]
#[serial]
fn never_overwrites_a_skill_the_user_put_there() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "deploy");
    // 用户自己在 Codex 目录里放了同名 skill
    env.write_skill(
        &env.codex_skills(),
        "deploy",
        "---\nname: deploy\n---\nMINE",
    );

    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let claude_skill = studio
        .scan_skills(&config)
        .unwrap()
        .into_iter()
        .find(|v| {
            matches!(v.skill.origin, SkillOrigin::InPlace { ref owner_agent } if owner_agent == "claude-code")
        })
        .unwrap();

    let report = studio
        .register(
            &mut config,
            std::slice::from_ref(&claude_skill.skill.id),
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();
    assert_eq!(report.failed.len(), 1, "应拒绝覆盖");
    assert!(report.failed[0].message.as_ref().unwrap().contains("占用"));
    // 用户的内容完好
    assert_eq!(
        std::fs::read_to_string(env.codex_skills().join("deploy/SKILL.md")).unwrap(),
        "---\nname: deploy\n---\nMINE"
    );
}

#[test]
#[serial]
fn registering_a_batch_leaves_other_registrations_untouched() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "batch-a");
    env.write_simple_skill(&env.claude_skills(), "batch-b");
    env.write_simple_skill(&env.claude_skills(), "outsider");

    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let views = studio.scan_skills(&config).unwrap();
    let id_of = |n: &str| {
        views
            .iter()
            .find(|v| v.skill.name == n)
            .unwrap()
            .skill
            .id
            .clone()
    };

    // 先把 outsider 单独注册到 codex —— 它不在这次批量里
    studio
        .register(
            &mut config,
            &[id_of("outsider")],
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();

    let report = studio
        .register(
            &mut config,
            &[id_of("batch-a"), id_of("batch-b")],
            &["codex".into()],
            None,
            false,
        )
        .unwrap();
    assert!(report.is_all_ok(), "{report:?}");
    assert_eq!(report.success.len(), 2);

    // 批量里的两个都到位，批量外那个没被动过
    assert!(env.codex_skills().join("batch-a").exists());
    assert!(env.codex_skills().join("batch-b").exists());
    assert!(
        env.codex_skills().join("outsider").exists(),
        "批量外的 skill 必须保持原状"
    );
}

#[test]
#[serial]
fn unregister_only_removes_requested_skills_and_spares_foreign() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "member");
    env.write_simple_skill(&env.claude_skills(), "kept");
    // 用户手工放在 codex 里的东西
    env.write_skill(
        &env.codex_skills(),
        "handmade",
        "---\nname: handmade\n---\n",
    );

    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let views = studio.scan_skills(&config).unwrap();
    let id_of = |n: &str| {
        views
            .iter()
            .find(|v| v.skill.name == n && matches!(v.skill.origin, SkillOrigin::InPlace { ref owner_agent } if owner_agent == "claude-code"))
            .unwrap()
            .skill
            .id
            .clone()
    };

    studio
        .register(
            &mut config,
            &[id_of("member"), id_of("kept")],
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();
    assert!(env.codex_skills().join("member").exists());

    let report = studio
        .unregister(
            &mut config,
            &[id_of("member")],
            &[String::from("codex")],
            false,
        )
        .unwrap();
    assert!(report.is_all_ok(), "{report:?}");

    assert!(
        !env.codex_skills().join("member").exists(),
        "请求的应被移除"
    );
    assert!(env.codex_skills().join("kept").exists(), "其他注册不该被动");
    assert!(
        env.codex_skills().join("handmade").exists(),
        "用户手工放的必须保留"
    );
}

#[test]
#[serial]
fn project_binding_writes_copies_to_both_agents() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "lint-rules");
    let project_root = env.path().join("work/webapp");
    std::fs::create_dir_all(&project_root).unwrap();

    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    let mut project = ProjectBinding::new("p1".into(), "webapp".into(), project_root.clone());
    project.agent_ids = vec!["claude-code".into(), "codex".into()];
    project.skill_ids = vec![id];
    config.projects.push(project);

    let report = studio.apply_project(&mut config, "p1").unwrap();
    assert!(report.is_all_ok(), "{report:?}");

    // Claude 用 .claude/skills，Codex 用 .agents/skills（不是 .codex/skills）
    let claude_dest = project_root.join(".claude/skills/lint-rules");
    let codex_dest = project_root.join(".agents/skills/lint-rules");
    assert!(claude_dest.join("SKILL.md").is_file());
    assert!(codex_dest.join("SKILL.md").is_file());

    // 项目级默认 Copy：必须是真文件而不是 symlink，否则进 git 就是死链
    assert!(!claude_dest
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(claude_dest.join(".skill-studio-copy.json").is_file());
}

#[test]
#[serial]
fn project_expands_bound_groups() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "a");
    env.write_simple_skill(&env.claude_skills(), "b");
    let project_root = env.path().join("work/api");
    std::fs::create_dir_all(&project_root).unwrap();

    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let views = studio.scan_skills(&config).unwrap();
    let id_of = |n: &str| {
        views
            .iter()
            .find(|v| v.skill.name == n)
            .unwrap()
            .skill
            .id
            .clone()
    };

    let mut group = Group::new("g1".into(), "通用".into());
    group.add_skill(id_of("a"));
    group.add_skill(id_of("b"));
    config.groups.push(group);

    let mut project = ProjectBinding::new("p1".into(), "api".into(), project_root.clone());
    project.agent_ids = vec!["claude-code".into()];
    project.group_ids = vec!["g1".into()];
    config.projects.push(project);

    let report = studio.apply_project(&mut config, "p1").unwrap();
    assert_eq!(report.success.len(), 2, "分组应被展开");
    assert!(project_root.join(".claude/skills/a").exists());
    assert!(project_root.join(".claude/skills/b").exists());
}

#[test]
#[serial]
fn native_toggle_is_reflected_in_the_view() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "deploy");
    let studio = env.studio();
    let config = studio.load_config().unwrap();

    // 未停用时 disabled 为 false
    let v = &studio.scan_skills(&config).unwrap()[0];
    assert!(!v.agents["claude-code"].disabled);

    let id = v.skill.id.clone();
    studio
        .set_skill_enabled(&config, &id, "claude-code", false)
        .unwrap();

    let v = &studio.scan_skills(&config).unwrap()[0];
    assert!(v.agents["claude-code"].disabled, "应反映为已停用");
    // 文件仍在 —— 停用不删文件
    assert!(env.claude_skills().join("deploy/SKILL.md").is_file());
    // 写进了 Claude 自己的 settings.json
    let settings = std::fs::read_to_string(env.path().join(".claude/settings.json")).unwrap();
    assert!(settings.contains("skillOverrides"), "{settings}");

    studio
        .set_skill_enabled(&config, &id, "claude-code", true)
        .unwrap();
    let v = &studio.scan_skills(&config).unwrap()[0];
    assert!(!v.agents["claude-code"].disabled);
}

#[test]
#[serial]
fn adopt_to_hub_moves_source_and_relinks_the_original_spot() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "shared-skill");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let old_id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    // 托管前先放进一个分组，验证引用会跟着迁移
    let mut group = Group::new("g1".into(), "通用".into());
    group.add_skill(old_id.clone());
    config.groups.push(group);

    let adopted = studio.adopt_to_hub(&mut config, &old_id).unwrap();
    assert!(matches!(adopted.origin, SkillOrigin::Hub));
    assert_eq!(adopted.source_path, env.hub().join("shared-skill"));
    assert!(adopted.source_path.join("SKILL.md").is_file());
    // Hub 里的真身不该留复制边车
    assert!(!adopted.source_path.join(".skill-studio-copy.json").exists());

    // 原位置变成了指向 Hub 的注册，Claude 依然能用
    let original = env.claude_skills().join("shared-skill");
    assert!(original.join("SKILL.md").is_file());

    // 分组里的引用已迁移到新 ID
    assert_ne!(adopted.id, old_id);
    assert!(config.groups[0].contains(&adopted.id));
    assert!(!config.groups[0].contains(&old_id));
}

#[test]
#[serial]
fn prune_drops_references_after_a_skill_disappears() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "temp");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    let mut group = Group::new("g1".into(), "通用".into());
    group.add_skill(id.clone());
    config.groups.push(group);
    studio
        .register(
            &mut config,
            std::slice::from_ref(&id),
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();

    // 用户在 Studio 之外把真身删了
    std::fs::remove_dir_all(env.claude_skills().join("temp")).unwrap();

    let pruned = studio.prune(&mut config).unwrap();
    assert!(pruned >= 1);
    assert!(config.groups[0].skill_ids.is_empty());
    assert!(config.registration(&id, "codex").is_none());
}

#[test]
#[serial]
fn copy_mode_registration_goes_stale_when_source_changes() {
    let env = Env::new();
    let src = env.write_simple_skill(&env.claude_skills(), "docs");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    config.settings.default_link_mode = LinkMode::Copy;
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    studio
        .register(
            &mut config,
            std::slice::from_ref(&id),
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();
    let v = studio.scan_skills(&config).unwrap();
    let v = v.iter().find(|v| v.skill.id == id).unwrap();
    assert_eq!(state(v, "codex"), LinkStatus::Copied);

    std::fs::write(src.join("SKILL.md"), "---\nname: docs\n---\nCHANGED").unwrap();
    let v = studio.scan_skills(&config).unwrap();
    let v = v.iter().find(|v| v.skill.id == id).unwrap();
    assert_eq!(state(v, "codex"), LinkStatus::CopyStale);

    // 重新注册后恢复
    studio
        .register(
            &mut config,
            std::slice::from_ref(&id),
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();
    let v = studio.scan_skills(&config).unwrap();
    let v = v.iter().find(|v| v.skill.id == id).unwrap();
    assert_eq!(state(v, "codex"), LinkStatus::Copied);
}

#[test]
#[serial]
fn unregister_never_deletes_the_source() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "keepme");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    studio
        .register(
            &mut config,
            std::slice::from_ref(&id),
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();
    studio
        .unregister(
            &mut config,
            std::slice::from_ref(&id),
            &[String::from("claude-code"), String::from("codex")],
            false,
        )
        .unwrap();

    // Codex 侧的注册被清掉，Claude 侧的真身必须还在
    assert!(!env.codex_skills().join("keepme").exists());
    assert!(
        env.claude_skills().join("keepme/SKILL.md").is_file(),
        "真身绝不能被当作注册删掉"
    );
}

#[test]
#[serial]
fn frontmatter_extra_keys_are_surfaced_for_cross_agent_lint() {
    let env = Env::new();
    env.write_skill(
        &env.claude_skills(),
        "forked",
        "---\nname: forked\ndescription: d\ncontext: fork\nagent: Explore\n---\n",
    );
    let studio = env.studio();
    let config = studio.load_config().unwrap();
    let v = &studio.scan_skills(&config).unwrap()[0];
    let mut extra = v.skill.frontmatter_extra.clone();
    extra.sort();
    // context / agent 是 Claude Code 专有，注册到 Codex 会被忽略
    assert_eq!(extra, vec!["agent", "context"]);
}

#[test]
#[serial]
fn config_survives_a_save_load_roundtrip() {
    let env = Env::new();
    env.write_simple_skill(&env.claude_skills(), "s1");
    let studio = env.studio();
    let mut config = studio.load_config().unwrap();
    let id = studio.scan_skills(&config).unwrap()[0].skill.id.clone();

    let mut group = Group::new("g1".into(), "前端".into());
    group.add_skill(id.clone());
    config.groups.push(group);
    studio
        .register(
            &mut config,
            std::slice::from_ref(&id),
            &[String::from("codex")],
            None,
            false,
        )
        .unwrap();
    studio.save_config(&config).unwrap();

    let reloaded = studio.load_config().unwrap();
    assert_eq!(reloaded.groups.len(), 1);
    assert!(reloaded.groups[0].contains(&id));
    assert!(reloaded.registration(&id, "codex").is_some());
}

#[test]
#[serial]
fn prune_refuses_missing_custom_hub() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    c.settings.hub_dir = Some(env.path().join("unmounted/skills"));
    let before = serde_json::to_value(&c).unwrap();
    assert!(studio.prune(&mut c).is_err());
    assert_eq!(serde_json::to_value(&c).unwrap(), before);
}

#[test]
#[serial]
fn registration_rolls_back_when_config_cannot_be_saved() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    // A directory at the config-file path is a deterministic write failure.
    std::fs::create_dir_all(studio.store().config_path()).unwrap();
    let before = serde_json::to_value(&c).unwrap();
    let result = studio.register(
        &mut c,
        &[id],
        &["codex".into()],
        Some(LinkMode::Copy),
        false,
    );
    assert!(result.is_err() || !result.unwrap().is_all_ok());
    assert!(!env.codex_skills().join("demo").exists());
    assert_eq!(serde_json::to_value(&c).unwrap(), before);
}

#[test]
#[serial]
fn failed_registration_repair_keeps_existing_copy() {
    let env = Env::new();
    let studio = env.studio();
    let mut c = studio.load_config().unwrap();
    let source = env.write_simple_skill(&env.hub(), "demo");
    let id = studio.scan_skills(&c).unwrap()[0].skill.id.clone();
    assert!(studio
        .register(
            &mut c,
            std::slice::from_ref(&id),
            &["codex".into()],
            Some(LinkMode::Copy),
            false
        )
        .unwrap()
        .is_all_ok());
    let dest = env.codex_skills().join("demo");
    let old = std::fs::read(dest.join("SKILL.md")).unwrap();
    std::fs::write(source.join("SKILL.md"), "updated source").unwrap();
    std::fs::remove_file(studio.store().config_path()).unwrap();
    std::fs::create_dir(studio.store().config_path()).unwrap();
    let before = serde_json::to_value(&c).unwrap();
    assert!(!studio
        .register(
            &mut c,
            &[id],
            &["codex".into()],
            Some(LinkMode::Copy),
            false
        )
        .unwrap()
        .is_all_ok());
    assert_eq!(std::fs::read(dest.join("SKILL.md")).unwrap(), old);
    assert_eq!(serde_json::to_value(&c).unwrap(), before);
}
