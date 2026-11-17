//! Explicit export of an approved package to a native client project.
use crate::{
    client::HostClient,
    error::{AppError, Result},
    skill::SkillPackage,
    storage::Scope,
};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum SkillClient {
    ClaudeCode,
    Codex,
}

impl SkillClient {
    fn project_config_dir(self) -> &'static str {
        match self {
            Self::ClaudeCode => ".claude",
            Self::Codex => ".agents",
        }
    }
}

/// Exports an immutable copy. This does not execute the client, enable tools,
/// or make an existing exported copy subject to live server revocation.
pub async fn export_skill(
    client: &HostClient,
    target: SkillClient,
    project: &Path,
    expected: Scope,
    skill: Uuid,
    version: u32,
) -> Result<PathBuf> {
    if skill.is_nil() || version == 0 {
        return Err(AppError::Invalid(
            "invalid skill identity or version".into(),
        ));
    }
    let path = format!("/v1/skills/{skill}/versions/{version}/package");
    let response = client.call(reqwest::Method::GET, &path, None, None).await?;
    let package: SkillPackage = serde_json::from_value(response)
        .map_err(|_| AppError::Unavailable("invalid skill package response".into()))?;
    package.validate()?;
    let manifest = &package.manifest;
    if manifest.tenant_id != expected.tenant_id
        || manifest.workspace_id != expected.workspace_id
        || manifest.skill_id != skill
        || manifest.version != version
    {
        return Err(AppError::Forbidden);
    }
    let project = project.to_owned();
    tokio::task::spawn_blocking(move || {
        let name = &package.manifest.name;
        let manifest = serde_json::to_vec(&package.manifest).map_err(anyhow::Error::from)?;
        install(target, &project, name, &package.files, &manifest)
    })
    .await
    .map_err(anyhow::Error::from)?
}

fn install(
    target: SkillClient,
    project: &Path,
    name: &str,
    files: &BTreeMap<String, String>,
    manifest: &[u8],
) -> Result<PathBuf> {
    let project = fs::canonicalize(project)
        .map_err(|_| AppError::Invalid("project must be an existing directory".into()))?;
    if !project.is_dir() {
        return Err(AppError::Invalid(
            "project must be an existing directory".into(),
        ));
    }
    let base = project.join(target.project_config_dir());
    let skill_root = base.join("skills");
    ensure_real_dir(&base)?;
    ensure_real_dir(&skill_root)?;
    let dest = skill_root.join(name);
    fs::create_dir(&dest).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            AppError::Conflict("skill folder already exists; export will not overwrite it".into())
        } else {
            export_io()
        }
    })?;

    // Assets were validated by SkillPackage::validate (only SKILL.md plus
    // direct text children of references/). Publish the native entry last.
    let write = || -> Result<()> {
        let entry = files.get("SKILL.md").ok_or_else(export_io)?;
        if files.len() > 1 {
            fs::create_dir(dest.join("references")).map_err(|_| export_io())?;
        }
        for (path, contents) in files.iter().filter(|(path, _)| *path != "SKILL.md") {
            write_new(&dest.join(path), contents.as_bytes())?;
        }
        write_new(&dest.join("origence-manifest.json"), manifest)?;
        let pending = dest.join(".origence-skill.pending");
        write_new(&pending, entry.as_bytes())?;
        fs::rename(pending, dest.join("SKILL.md")).map_err(|_| export_io())
    };
    if let Err(error) = write() {
        let _ = fs::remove_dir_all(&dest);
        return Err(error);
    }
    Ok(dest)
}

fn ensure_real_dir(path: &Path) -> Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(path).map_err(|_| export_io())?;
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                Ok(())
            } else {
                Err(AppError::Conflict(
                    "skill directory is not a real directory".into(),
                ))
            }
        }
        Err(_) => Err(export_io()),
    }
}

fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = File::create_new(path).map_err(|_| export_io())?;
    file.write_all(data).map_err(|_| export_io())?;
    file.sync_all().map_err(|_| export_io())
}

fn export_io() -> AppError {
    AppError::Unavailable("could not export skill to the project".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_bytes_to_both_native_layouts_without_overwriting() {
        let project = tempfile::tempdir().unwrap();
        let files = BTreeMap::from([
            ("SKILL.md".into(), "approved body\n".into()),
            (
                "references/runbook.txt".into(),
                "approved reference\n".into(),
            ),
        ]);
        for (client, path) in [
            (SkillClient::ClaudeCode, ".claude/skills/runbook"),
            (SkillClient::Codex, ".agents/skills/runbook"),
        ] {
            let dest = install(client, project.path(), "runbook", &files, b"{}").unwrap();
            assert!(dest.ends_with(path));
            assert_eq!(fs::read(dest.join("SKILL.md")).unwrap(), b"approved body\n");
            assert_eq!(
                fs::read(dest.join("references/runbook.txt")).unwrap(),
                b"approved reference\n"
            );
            assert!(matches!(
                install(client, project.path(), "runbook", &files, b"{}"),
                Err(AppError::Conflict(_))
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_redirected_client_directory() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), project.path().join(".agents")).unwrap();
        assert!(matches!(
            install(
                SkillClient::Codex,
                project.path(),
                "runbook",
                &BTreeMap::new(),
                b"{}"
            ),
            Err(AppError::Conflict(_))
        ));
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }
}
