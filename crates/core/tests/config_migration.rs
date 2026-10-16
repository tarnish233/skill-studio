//! v4 及更早的配置里每个 agent 只存一个分组对象；现在可以叠加多个。
//! 旧形状必须能直接读进来（读成单元素列表），否则升级后所有启用记录都会丢失。
use skill_studio_core::models::config::AppConfig;

#[test]
fn old_single_active_group_object_loads_as_a_one_element_stack() {
    let config: AppConfig = serde_json::from_str(
        r#"{
            "version": 4,
            "groups": [{"id":"g","name":"G","skillIds":["s"]}],
            "activeGroups": {
                "codex": {
                    "groupId": "g",
                    "skillIds": ["s"],
                    "entries": [
                        {"skillId":"s","sourcePath":"/a","targetPath":"/b"}
                    ],
                    "suspendedManual": [],
                    "preserveManualSkills": true
                }
            }
        }"#,
    )
    .expect("旧版单个分组对象必须能读成单元素列表");
    let records = config.active_groups_of("codex");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].group_id, "g");
    assert_eq!(config.active_skill_ids("codex"), vec!["s".to_string()]);
    assert_eq!(config.active_entries("codex").len(), 1);

    // 新形状（列表）同样可读，并且叠加后成员取并集
    let stacked: AppConfig = serde_json::from_str(
        r#"{
            "version": 5,
            "activeGroups": {
                "codex": [
                    {"groupId":"g1","skillIds":["a","s"],"entries":[]},
                    {"groupId":"g2","skillIds":["b","s"],"entries":[]}
                ]
            }
        }"#,
    )
    .unwrap();
    assert_eq!(stacked.active_groups_of("codex").len(), 2);
    assert_eq!(
        stacked.active_skill_ids("codex"),
        vec!["a".to_string(), "s".to_string(), "b".to_string()]
    );
    assert_eq!(stacked.agents_using_group("g2"), vec!["codex"]);
}
