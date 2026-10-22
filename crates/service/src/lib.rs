pub mod agents;
pub mod groups;
pub mod mcp;
pub mod projects;
pub mod settings;
pub mod skills;
pub mod state;
use serde_json::Value;
use state::AppState;
fn arg<T: serde::de::DeserializeOwned>(p: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(p.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| format!("参数 {key}: {e}"))
}
pub fn dispatch(state: &AppState, method: &str, params: Value) -> Result<Value, String> {
    let result = match method {
        "list_agents" => {
            serde_json::to_value(agents::list_agents(state, arg(&params, "skipCliProbe")?)?)
        }
        "activate_agent_group" => serde_json::to_value(groups::activate_agent_groups(
            state,
            arg(&params, "agentId")?,
            arg(&params, "groupIds")?,
        )?),
        "delete_group" => {
            serde_json::to_value(groups::delete_group(state, arg(&params, "groupId")?)?)
        }
        "list_groups" => serde_json::to_value(groups::list_groups(state)?),
        "reorder_groups" => {
            serde_json::to_value(groups::reorder_groups(state, arg(&params, "groupIds")?)?)
        }
        "save_group" => serde_json::to_value(groups::save_group(
            state,
            arg(&params, "groupId")?,
            arg(&params, "name")?,
            arg(&params, "skillIds")?,
            arg(&params, "expected")?,
        )?),
        "set_agent_group" => serde_json::to_value(groups::set_agent_group(
            state,
            arg(&params, "agentId")?,
            arg(&params, "groupId")?,
            arg(&params, "enabled")?,
        )?),
        "reapply_agent_groups" => serde_json::to_value(groups::reapply_agent_groups(
            state,
            arg(&params, "agentId")?,
        )?),
        "apply_project" => serde_json::to_value(projects::apply_project(
            state,
            arg(&params, "projectId")?,
            arg(&params, "selection")?,
        )?),
        "collect_project_skill" => serde_json::to_value(projects::collect_project_skill(
            state,
            arg(&params, "projectId")?,
            arg(&params, "path")?,
        )?),
        "create_project" => serde_json::to_value(projects::create_project(
            state,
            arg(&params, "name")?,
            arg(&params, "root")?,
        )?),
        "delete_project" => {
            serde_json::to_value(projects::delete_project(state, arg(&params, "projectId")?)?)
        }
        "delete_project_local_skill" => serde_json::to_value(projects::delete_project_local_skill(
            state,
            arg(&params, "projectId")?,
            arg(&params, "path")?,
        )?),
        "delete_skill_file" => serde_json::to_value(projects::delete_skill_file(
            state,
            arg(&params, "scope")?,
            arg(&params, "path")?,
        )?),
        "list_project_skills" => serde_json::to_value(projects::list_project_skills(
            state,
            arg(&params, "projectId")?,
        )?),
        "list_projects" => serde_json::to_value(projects::list_projects(state)?),
        "list_skill_backups" => serde_json::to_value(projects::list_skill_backups(state)?),
        "purge_skill_file" => {
            serde_json::to_value(projects::purge_skill_file(state, arg(&params, "id")?)?)
        }
        "reorder_projects" => serde_json::to_value(projects::reorder_projects(
            state,
            arg(&params, "projectIds")?,
        )?),
        "restore_skill_file" => {
            serde_json::to_value(projects::restore_skill_file(state, arg(&params, "id")?)?)
        }
        "set_project_enabled" => serde_json::to_value(projects::set_project_enabled(
            state,
            arg(&params, "projectId")?,
            arg(&params, "enabled")?,
        )?),
        "set_project_agent" => serde_json::to_value(projects::set_project_agent(
            state,
            arg(&params, "projectId")?,
            arg(&params, "agentId")?,
            arg(&params, "enabled")?,
        )?),
        "reapply_project" => serde_json::to_value(projects::reapply_project(
            state,
            arg(&params, "projectId")?,
        )?),
        "save_project" => serde_json::to_value(projects::save_project(
            state,
            arg(&params, "projectId")?,
            arg(&params, "members")?,
            arg(&params, "expected")?,
        )?),
        "set_project_skill_enabled" => serde_json::to_value(projects::set_project_skill_enabled(
            state,
            arg(&params, "projectId")?,
            arg(&params, "path")?,
            arg(&params, "enabled")?,
        )?),
        "unapply_project" => serde_json::to_value(projects::unapply_project(
            state,
            arg(&params, "projectId")?,
            arg(&params, "skillIds")?,
            arg(&params, "force")?,
        )?),
        "update_project" => serde_json::to_value(projects::update_project(
            state,
            arg(&params, "projectId")?,
            arg(&params, "name")?,
            arg(&params, "agentIds")?,
            arg(&params, "skillIds")?,
            arg(&params, "groupIds")?,
            arg(&params, "linkMode")?,
        )?),
        "write_project_gitignore" => serde_json::to_value(projects::write_project_gitignore(
            state,
            arg(&params, "projectId")?,
        )?),
        "get_config" => serde_json::to_value(settings::get_config(state)?),
        "get_config_dir" => serde_json::to_value(settings::get_config_dir(state)?),
        "get_settings" => serde_json::to_value(settings::get_settings(state)?),
        "list_backups" => serde_json::to_value(settings::list_backups(state)?),
        "delete_backup" => {
            serde_json::to_value(settings::delete_backup(state, arg(&params, "path")?)?)
        }
        "list_hub_backups" => serde_json::to_value(skills::list_hub_backups(state)?),
        "delete_hub_backups" => {
            serde_json::to_value(skills::delete_hub_backups(state, arg(&params, "ids")?)?)
        }
        "restore_backup" => {
            serde_json::to_value(settings::restore_backup(state, arg(&params, "path")?)?)
        }
        "update_settings" => {
            serde_json::to_value(settings::update_settings(state, arg(&params, "patch")?)?)
        }
        "adopt_to_hub" => {
            serde_json::to_value(skills::adopt_to_hub(state, arg(&params, "skillId")?)?)
        }
        "discover_local_skills" => {
            serde_json::to_value(skills::discover_local_skills(arg(&params, "path")?)?)
        }
        "import_local_skill" => {
            serde_json::to_value(skills::import_local_skill(state, arg(&params, "path")?)?)
        }
        "install_catalog_skill" => serde_json::to_value(skills::install_catalog_skill(
            state,
            arg(&params, "source")?,
            arg(&params, "skillId")?,
            arg(&params, "repositoryPath")?,
            arg(&params, "repositoryPaths")?,
        )?),
        "prune_missing" => serde_json::to_value(skills::prune_missing(state)?),
        "read_skill_document" => {
            serde_json::to_value(skills::read_skill_document(state, arg(&params, "path")?)?)
        }
        "register_skills" => serde_json::to_value(skills::register_skills(
            state,
            arg(&params, "skillIds")?,
            arg(&params, "agentIds")?,
            arg(&params, "mode")?,
            arg(&params, "force")?,
        )?),
        "release_from_hub" => {
            serde_json::to_value(skills::release_from_hub(state, arg(&params, "skillId")?)?)
        }
        "scan_skills" => serde_json::to_value(skills::scan_skills(state)?),
        "search_catalog_skills" => {
            serde_json::to_value(skills::search_catalog_skills(arg(&params, "query")?)?)
        }
        "set_skill_enabled" => serde_json::to_value(skills::set_skill_enabled(
            state,
            arg(&params, "skillId")?,
            arg(&params, "agentId")?,
            arg(&params, "enabled")?,
        )?),
        "unregister_skills" => serde_json::to_value(skills::unregister_skills(
            state,
            arg(&params, "skillIds")?,
            arg(&params, "agentIds")?,
            arg(&params, "force")?,
        )?),
        _ => return Err(format!("不支持的管理命令: {method}")),
    };
    result.map_err(|e| e.to_string())
}
