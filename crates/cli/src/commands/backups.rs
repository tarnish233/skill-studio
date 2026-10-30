//! 四种备份使用各自的校验与恢复规则，不直接删除任意路径。
use super::Ctx;
use crate::{
    args::{BackupKind, BackupsCommand},
    error::CliError,
    report::Report,
};
use serde_json::{json, Value};
use skill_studio_service::{mcp, projects, settings, skills, state::AppState};

fn listing(state: &AppState, kind: BackupKind) -> Result<Value, CliError> {
    Ok(match kind {
        BackupKind::Config => {
            json!({"kind":"config","backups":settings::list_backups(state)?.iter().map(|path| json!({"id":std::path::Path::new(path).file_name().unwrap_or_default().to_string_lossy(),"path":path})).collect::<Vec<_>>()})
        }
        BackupKind::Skill => json!({"kind":"skill","backups":projects::list_skill_backups(state)?}),
        BackupKind::Hub => json!({"kind":"hub","backups":skills::list_hub_backups(state)?}),
        BackupKind::Mcp => {
            let found = mcp::backups::list(state)?;
            json!({"kind":"mcp","backups":found.backups,"legacy":found.legacy})
        }
    })
}
fn report_list(value: Value) -> Report {
    let mut text = format!(
        "{} 备份：{} 份",
        value["kind"].as_str().unwrap_or(""),
        value["backups"].as_array().map_or(0, Vec::len)
    );
    for item in value["backups"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "\n  {}{}{}",
            item["id"].as_str().unwrap_or(""),
            if item["inUse"] == true {
                " · 使用中，不能删除"
            } else {
                ""
            },
            item["status"]
                .as_str()
                .map(|s| format!(" · {s}"))
                .unwrap_or_default()
        ));
        if let Some(reason) = item["reason"].as_str() {
            text.push_str(&format!(" · {reason}"));
        }
    }
    Report::new(text, value)
}
fn selected<'a>(items: &'a [Value], ids: &[String]) -> Result<Vec<&'a Value>, CliError> {
    let mut result = vec![];
    for id in ids {
        let item = items
            .iter()
            .find(|item| item["id"].as_str() == Some(id) || item["path"].as_str() == Some(id))
            .ok_or_else(|| CliError::not_found("备份不存在，请先列出备份并指定 ID"))?;
        if !result.contains(&item) {
            result.push(item);
        }
    }
    Ok(result)
}
pub fn run(command: &BackupsCommand, ctx: &Ctx) -> Result<Report, CliError> {
    match command {
        BackupsCommand::List { kind: Some(kind) } => {
            let state = if *kind == BackupKind::Config {
                ctx.config_backups(false)?
            } else {
                ctx.reader()?
            };
            Ok(report_list(listing(&state, *kind)?))
        }
        BackupsCommand::List { kind: None } => {
            let state = ctx.reader()?;
            let mut reports = vec![];
            for kind in [
                BackupKind::Config,
                BackupKind::Skill,
                BackupKind::Hub,
                BackupKind::Mcp,
            ] {
                if kind == BackupKind::Mcp && !state.config().settings.manage_mcp {
                    continue;
                }
                reports.push(report_list(listing(&state, kind)?));
            }
            Ok(Report::new(
                reports
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                json!(reports.iter().map(|r| &r.json).collect::<Vec<_>>()),
            ))
        }
        BackupsCommand::Restore { kind, id } => {
            if *kind == BackupKind::Hub {
                return Err(CliError::usage(
                    "托管完整备份不能单独恢复；用 skills release 按原始来源还原",
                ));
            }
            ctx.confirm("恢复备份会修改当前文件或配置，需要 --yes 确认")?;
            let state = if *kind == BackupKind::Config {
                ctx.config_backups(true)?
            } else {
                ctx.writer()?
            };
            let found = listing(&state, *kind)?;
            let items = found["backups"].as_array().expect("backup list");
            let chosen = selected(items, std::slice::from_ref(id))?[0];
            match kind {
                BackupKind::Config => {
                    settings::restore_backup(&state, chosen["path"].as_str().unwrap().into())?;
                }
                BackupKind::Skill => {
                    projects::restore_skill_file(&state, chosen["id"].as_str().unwrap().into())?
                }
                BackupKind::Mcp => {
                    mcp::backups::restore(&state, id)?;
                    super::mcp::notify_gateway(&mcp::dir(&state));
                }
                BackupKind::Hub => unreachable!(),
            }
            Ok(Report::write(
                format!("已恢复 {} 备份 {id}", kind.name()),
                true,
                *kind != BackupKind::Config,
                &[],
                json!({"kind":kind.name(),"id":id}),
            ))
        }
        BackupsCommand::Delete {
            kind,
            ids,
            all,
            unused,
            unrestorable,
            legacy,
        } => {
            if (*unused && *kind != BackupKind::Hub)
                || ((*unrestorable || *legacy) && *kind != BackupKind::Mcp)
            {
                return Err(CliError::usage(
                    "--unused 仅用于 hub；--unrestorable / --legacy 仅用于 mcp",
                ));
            }
            if ids.is_empty() && !(*all || *unused || *unrestorable || *legacy) {
                return Err(CliError::usage("请指定备份 ID 或明确的删除范围"));
            }
            ctx.confirm("删除备份后无法恢复，需要 --yes 确认")?;
            let state = ctx.writer()?;
            let removed = if *kind == BackupKind::Mcp {
                if *legacy {
                    mcp::backups::delete_legacy(&state)?
                } else {
                    mcp::backups::delete(
                        &state,
                        if *all {
                            Some("all")
                        } else if *unrestorable {
                            Some("unrestorable")
                        } else {
                            None
                        },
                        Some(ids.clone()),
                    )?
                }
            } else {
                let found = listing(&state, *kind)?;
                let items = found["backups"].as_array().expect("backup list");
                let chosen = if *all {
                    items.iter().collect()
                } else if *unused {
                    items.iter().filter(|i| i["inUse"] != true).collect()
                } else {
                    selected(items, ids)?
                };
                if chosen.iter().any(|i| i["inUse"] == true) {
                    return Err(CliError::failed("选中的完整备份仍在使用，整批未删除"));
                }
                let ids: Vec<_> = chosen
                    .iter()
                    .map(|i| i["id"].as_str().unwrap().to_string())
                    .collect();
                match kind {
                    BackupKind::Hub => skills::delete_hub_backups(&state, ids)?,
                    BackupKind::Config => {
                        for item in &chosen {
                            settings::delete_backup(&state, item["path"].as_str().unwrap().into())?;
                        }
                        chosen.len()
                    }
                    BackupKind::Skill => {
                        for id in &ids {
                            projects::purge_skill_file(&state, id.clone())?;
                        }
                        ids.len()
                    }
                    BackupKind::Mcp => unreachable!(),
                }
            };
            Ok(Report::write(
                format!("已删除 {removed} 份 {} 备份", kind.name()),
                removed > 0,
                false,
                &[],
                json!({"kind":kind.name(),"removed":removed}),
            ))
        }
    }
}
