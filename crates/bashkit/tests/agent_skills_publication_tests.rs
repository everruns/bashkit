// Decision: assert the committed public skill inventory so private agent
// workflow skills stay hidden from `npx skills add everruns/bashkit`.

use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn public_skill_inventory_is_only_bashkit() {
    let repo = repo_root();
    let mut public_skills = find_skill_mds(&repo)
        .into_iter()
        .filter_map(|path| {
            let skill = parse_skill_md(&path);
            (!skill.internal).then(|| (repo_relative(&repo, &path), skill.name))
        })
        .collect::<Vec<_>>();

    public_skills.sort();

    assert_eq!(
        public_skills,
        vec![("skills/bashkit/SKILL.md".to_string(), "bashkit".to_string())],
        "only the public bashkit skill should be visible to default skill discovery"
    );
}

#[test]
fn workflow_agent_skills_are_internal() {
    let repo = repo_root();
    let mut workflow_skills = find_skill_mds(&repo)
        .into_iter()
        .filter(|path| repo_relative(&repo, path).starts_with(".agents/skills/"))
        .map(|path| {
            let rel = repo_relative(&repo, &path);
            let skill = parse_skill_md(&path);
            (rel, skill.internal)
        })
        .collect::<Vec<_>>();

    workflow_skills.sort();

    assert!(
        !workflow_skills.is_empty(),
        "expected committed workflow skills under .agents/skills/"
    );

    let public = workflow_skills
        .iter()
        .filter_map(|(rel, internal)| (!internal).then_some(rel.as_str()))
        .collect::<Vec<_>>();

    assert!(
        public.is_empty(),
        "workflow skills must set metadata.internal: true: {public:?}"
    );
}

#[derive(Debug)]
struct SkillFrontmatter {
    name: String,
    internal: bool,
}

fn parse_skill_md(path: &Path) -> SkillFrontmatter {
    let text = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", path.display());
    });

    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("---"),
        "{} must start with YAML frontmatter",
        path.display()
    );

    let mut name = None;
    let mut in_metadata = false;
    let mut internal = false;

    for line in lines {
        if line == "---" {
            break;
        }

        if let Some(value) = line.strip_prefix("name:") {
            name = Some(value.trim().trim_matches('"').to_string());
            in_metadata = false;
            continue;
        }

        if line == "metadata:" {
            in_metadata = true;
            continue;
        }

        if !line.starts_with(' ') && !line.is_empty() {
            in_metadata = false;
        }

        if in_metadata && line.trim() == "internal: true" {
            internal = true;
        }
    }

    SkillFrontmatter {
        name: name.unwrap_or_else(|| panic!("{} is missing name frontmatter", path.display())),
        internal,
    }
}

fn find_skill_mds(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, &mut found);
    found
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|err| {
        panic!("failed to read dir {}: {err}", dir.display());
    }) {
        let entry = entry.unwrap_or_else(|err| panic!("failed to read dir entry: {err}"));
        let path = entry.path();
        let file_name = entry.file_name();

        if entry
            .file_type()
            .unwrap_or_else(|err| panic!("failed to stat {}: {err}", path.display()))
            .is_dir()
        {
            if matches!(
                file_name.to_str(),
                Some(".git" | "target" | "node_modules" | ".venv" | "dist")
            ) {
                continue;
            }
            walk(&path, found);
        } else if file_name == "SKILL.md" {
            found.push(path);
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/bashkit should be two levels below repo root")
        .to_path_buf()
}

fn repo_relative(repo: &Path, path: &Path) -> String {
    path.strip_prefix(repo)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
