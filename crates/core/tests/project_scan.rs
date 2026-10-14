use skill_studio_core::services::scanner::scan_project;
use std::fs;

#[test]
fn discovers_repository_skills_without_bindings_or_writing_files() {
    let temp = tempfile::tempdir().unwrap();
    assert!(scan_project(temp.path()).unwrap().is_empty());
    for folder in [".agents/skills/release", ".claude/skills/review"] {
        let path = temp.path().join(folder);
        fs::create_dir_all(&path).unwrap();
        fs::write(
            path.join("SKILL.md"),
            "---\nname: demo\ndescription: Local skill\n---\nInstructions\n",
        )
        .unwrap();
    }
    fs::create_dir_all(temp.path().join(".agents/skills/not-a-skill")).unwrap();
    let entries = scan_project(temp.path()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "release");
    assert_eq!(entries[0].agent_id, "codex");
    assert_eq!(entries[1].agent_id, "claude-code");
    for entry in entries {
        assert_eq!(
            entry.frontmatter.unwrap().description.as_deref(),
            Some("Local skill")
        );
        assert_eq!(fs::read_dir(entry.path).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[test]
fn reports_broken_links_without_following_cycles() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(".agents/skills");
    fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink("missing", root.join("broken")).unwrap();
    std::os::unix::fs::symlink("cycle", root.join("cycle")).unwrap();
    let entries = scan_project(temp.path()).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.frontmatter.is_none()));
}

#[test]
fn preview_reads_document_and_rejects_missing_invalid_and_oversized_files() {
    use skill_studio_core::services::scanner::read_skill_document;
    let temp = tempfile::tempdir().unwrap();
    assert!(read_skill_document(temp.path()).is_err());
    let path = temp.path().join("SKILL.md");
    fs::write(&path, "---\nname: preview\n---\n# Preview\n正文").unwrap();
    let document = read_skill_document(temp.path()).unwrap();
    assert_eq!(document.frontmatter.name.as_deref(), Some("preview"));
    assert_eq!(document.body, "# Preview\n正文");
    fs::write(&path, [0xff]).unwrap();
    assert!(read_skill_document(temp.path()).is_err());
    fs::write(&path, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    assert!(read_skill_document(temp.path()).is_err());
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(read_skill_document(temp.path()).is_err());
}
