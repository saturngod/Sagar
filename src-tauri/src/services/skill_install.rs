//! Installs translation skills into each coding CLI's own skills folder, so
//! the same rules are available when the CLI is used outside Sagar.
//!
//! Sagar only ever changes folders it created: each installed folder holds a
//! `.sagar-skill.json` marker with the skill's version and a SHA-256 of the
//! installed `SKILL.md`. Folders without a marker are never replaced or
//! removed, and changed folders need an explicit `force`.
use crate::domain::CliProviderId;
use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MARKER: &str = ".sagar-skill.json";
const SKILL_FILE: &str = "SKILL.md";

/// Where the CLIs keep user-level skills.
#[derive(Debug, Clone)]
pub struct Roots {
    pub home: PathBuf,
    /// `XDG_CONFIG_HOME`, when set to an absolute path (used by OpenCode).
    pub xdg_config: Option<PathBuf>,
}

impl Roots {
    /// Current user's folders; errors when the home folder is unknown.
    pub fn current() -> Result<Self, AppError> {
        let home = crate::services::ai::home_directory().ok_or_else(|| {
            AppError::new(
                "home_not_found",
                "Your home folder could not be found, so skills can't be installed.",
            )
        })?;
        let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute());
        Ok(Self { home, xdg_config })
    }

    /// The CLI's user-level skills folder (documented locations).
    pub fn skills_dir(&self, provider: CliProviderId) -> PathBuf {
        match provider {
            CliProviderId::ClaudeCode => self.home.join(".claude").join("skills"),
            // Codex reads user skills from ~/.agents/skills.
            CliProviderId::Codex => self.home.join(".agents").join("skills"),
            CliProviderId::Opencode => self
                .xdg_config
                .clone()
                .unwrap_or_else(|| self.home.join(".config"))
                .join("opencode")
                .join("skills"),
            CliProviderId::Qoder => self.home.join(".qoder").join("skills"),
            // Shared by the Antigravity app and the agy CLI.
            CliProviderId::Agy => self.home.join(".gemini").join("config").join("skills"),
        }
    }

    /// Folder shown to the user, with the home folder written as `~`.
    fn display(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallStatus {
    NotInstalled,
    Installed,
    UpdateAvailable,
    /// Installed by Sagar, then changed outside Sagar.
    Modified,
    /// A folder with this name exists but wasn't installed by Sagar.
    Conflict,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstall {
    pub provider_id: String,
    pub label: String,
    pub folder: String,
    pub detected: bool,
    pub status: InstallStatus,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Marker {
    installed_by: String,
    skill_id: String,
    version: String,
    sha256: String,
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

fn status_of(dir: &Path, skill_id: &str, current: &str) -> InstallStatus {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return InstallStatus::NotInstalled;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return InstallStatus::Conflict;
    }
    let marker = std::fs::read(dir.join(MARKER))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok())
        .filter(|m| m.installed_by == "Sagar" && m.skill_id == skill_id);
    let Some(marker) = marker else {
        return InstallStatus::Conflict;
    };
    let file = dir.join(SKILL_FILE);
    if is_symlink(&file) {
        return InstallStatus::Modified;
    }
    let Ok(installed) = std::fs::read(&file) else {
        return InstallStatus::Modified;
    };
    let extra = std::fs::read_dir(dir).map_or(true, |entries| {
        entries
            .filter_map(Result::ok)
            .any(|e| e.file_name() != SKILL_FILE && e.file_name() != MARKER)
    });
    if extra || sha256(&installed) != marker.sha256 {
        return InstallStatus::Modified;
    }
    if installed != current.as_bytes() {
        return InstallStatus::UpdateAvailable;
    }
    InstallStatus::Installed
}

fn provider(id: &str) -> Result<CliProviderId, AppError> {
    CliProviderId::parse(id)
        .ok_or_else(|| AppError::new("invalid_provider", "Choose a supported coding CLI."))
}

/// Install status of one skill for every supported CLI.
pub fn list(
    data_dir: &Path,
    roots: &Roots,
    detected: &[CliProviderId],
    skill_id: &str,
) -> Result<Vec<SkillInstall>, AppError> {
    let current = crate::services::skills::source(data_dir, skill_id)?;
    Ok(CliProviderId::all()
        .into_iter()
        .map(|id| {
            let root = roots.skills_dir(id);
            SkillInstall {
                provider_id: id.as_str().to_string(),
                label: id.label().to_string(),
                folder: roots.display(&root),
                detected: detected.contains(&id),
                status: status_of(&root.join(skill_id), skill_id, &current),
            }
        })
        .collect())
}

fn refuse_conflict() -> AppError {
    AppError::new(
        "skill_install_conflict",
        "A skill folder with this name already exists and wasn't installed by Sagar. Sagar won't change it.",
    )
}

fn refuse_modified() -> AppError {
    AppError::new(
        "skill_install_modified",
        "This installed skill was changed outside Sagar. Confirm to replace or remove those changes.",
    )
}

/// Installs or updates a skill for one CLI. Files are written to a temporary
/// folder next to the destination and then moved into place.
pub fn install(
    data_dir: &Path,
    roots: &Roots,
    skill_id: &str,
    provider_id: &str,
    force: bool,
) -> Result<InstallStatus, AppError> {
    let id = provider(provider_id)?;
    let skill = crate::services::skills::get(data_dir, skill_id).ok_or_else(|| {
        AppError::new(
            "skill_not_found",
            "This translation skill is no longer available.",
        )
    })?;
    let text = crate::services::skills::source(data_dir, skill_id)?;
    let root = roots.skills_dir(id);
    // skill IDs are validated lowercase-dash names, so this stays inside root.
    let target = root.join(&skill.summary.id);
    match status_of(&target, skill_id, &text) {
        InstallStatus::Conflict => return Err(refuse_conflict()),
        InstallStatus::Modified if !force => return Err(refuse_modified()),
        _ => {}
    }
    std::fs::create_dir_all(&root)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let temp = root.join(format!(".sagar-tmp-{token}"));
    let write = || -> Result<(), AppError> {
        std::fs::create_dir(&temp)?;
        std::fs::write(temp.join(SKILL_FILE), text.as_bytes())?;
        let marker = Marker {
            installed_by: "Sagar".into(),
            skill_id: skill.summary.id.clone(),
            version: skill.summary.version.clone(),
            sha256: sha256(text.as_bytes()),
        };
        std::fs::write(
            temp.join(MARKER),
            serde_json::to_vec_pretty(&marker).map_err(|_| {
                AppError::new("skill_install_failed", "The skill could not be installed.")
            })?,
        )?;
        Ok(())
    };
    if let Err(error) = write() {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(error);
    }
    if std::fs::symlink_metadata(&target).is_ok() {
        let old = root.join(format!(".sagar-old-{token}"));
        std::fs::rename(&target, &old).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&temp);
        })?;
        if let Err(error) = std::fs::rename(&temp, &target) {
            let _ = std::fs::rename(&old, &target);
            let _ = std::fs::remove_dir_all(&temp);
            return Err(error.into());
        }
        let _ = std::fs::remove_dir_all(&old);
    } else if let Err(error) = std::fs::rename(&temp, &target) {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(error.into());
    }
    Ok(InstallStatus::Installed)
}

/// Removes a skill Sagar installed for one CLI.
pub fn uninstall(
    data_dir: &Path,
    roots: &Roots,
    skill_id: &str,
    provider_id: &str,
    force: bool,
) -> Result<InstallStatus, AppError> {
    let id = provider(provider_id)?;
    if !crate::services::skills::is_valid_id(skill_id) {
        return Err(AppError::new(
            "skill_not_found",
            "This translation skill is no longer available.",
        ));
    }
    // A deleted skill can still be uninstalled; its text only matters for
    // telling an up-to-date install from an outdated one.
    let current = crate::services::skills::source(data_dir, skill_id).unwrap_or_default();
    let target = roots.skills_dir(id).join(skill_id);
    match status_of(&target, skill_id, &current) {
        InstallStatus::NotInstalled => return Ok(InstallStatus::NotInstalled),
        InstallStatus::Conflict => return Err(refuse_conflict()),
        InstallStatus::Modified if !force => return Err(refuse_modified()),
        _ => {}
    }
    std::fs::remove_dir_all(&target)?;
    Ok(InstallStatus::NotInstalled)
}

/// Removes unchanged Sagar installs of a skill from every CLI. Returns the
/// CLIs that still have a copy (changed outside Sagar, or not Sagar's).
pub fn uninstall_everywhere(data_dir: &Path, roots: &Roots, skill_id: &str) -> Vec<String> {
    CliProviderId::all()
        .into_iter()
        .filter(|id| uninstall(data_dir, roots, skill_id, id.as_str(), false).is_err())
        .filter(|id| roots.skills_dir(*id).join(skill_id).exists())
        .map(|id| id.label().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, Roots) {
        let data = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let roots = Roots {
            home: home.path().to_path_buf(),
            xdg_config: None,
        };
        (data, home, roots)
    }

    fn status(data: &Path, roots: &Roots, skill: &str, provider: &str) -> InstallStatus {
        list(data, roots, &[], skill)
            .unwrap()
            .into_iter()
            .find(|i| i.provider_id == provider)
            .unwrap()
            .status
    }

    #[test]
    fn folders_follow_each_clis_documented_location() {
        let roots = Roots {
            home: PathBuf::from("/home/u"),
            xdg_config: None,
        };
        let folder = |id| roots.display(&roots.skills_dir(id));
        assert_eq!(folder(CliProviderId::ClaudeCode), "~/.claude/skills");
        assert_eq!(folder(CliProviderId::Codex), "~/.agents/skills");
        assert_eq!(folder(CliProviderId::Opencode), "~/.config/opencode/skills");
        assert_eq!(folder(CliProviderId::Qoder), "~/.qoder/skills");
        assert_eq!(folder(CliProviderId::Agy), "~/.gemini/config/skills");
        let xdg = Roots {
            xdg_config: Some(PathBuf::from("/xdg")),
            ..roots
        };
        assert_eq!(
            xdg.skills_dir(CliProviderId::Opencode),
            PathBuf::from("/xdg/opencode/skills")
        );
    }

    #[test]
    fn install_writes_skill_and_marker_then_uninstalls() {
        let (data, home, roots) = setup();
        let installs = list(data.path(), &roots, &[CliProviderId::Qoder], "sagar-wuxia").unwrap();
        assert_eq!(installs.len(), 5);
        assert!(
            installs
                .iter()
                .all(|i| i.status == InstallStatus::NotInstalled)
        );
        assert!(
            installs
                .iter()
                .any(|i| i.provider_id == "qoder" && i.detected)
        );
        assert_eq!(
            install(data.path(), &roots, "sagar-wuxia", "qoder", false).unwrap(),
            InstallStatus::Installed
        );
        let dir = home.path().join(".qoder/skills/sagar-wuxia");
        assert_eq!(
            std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
            crate::services::skills::source(data.path(), "sagar-wuxia").unwrap()
        );
        assert!(dir.join(MARKER).is_file());
        assert_eq!(
            status(data.path(), &roots, "sagar-wuxia", "qoder"),
            InstallStatus::Installed
        );
        // Reinstalling is safe and leaves no temporary folders behind.
        install(data.path(), &roots, "sagar-wuxia", "qoder", false).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(home.path().join(".qoder/skills"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("sagar-wuxia")]);
        uninstall(data.path(), &roots, "sagar-wuxia", "qoder", false).unwrap();
        assert!(!dir.exists());
        assert_eq!(
            uninstall(data.path(), &roots, "sagar-wuxia", "qoder", false).unwrap(),
            InstallStatus::NotInstalled
        );
    }

    #[test]
    fn edited_skill_shows_update_available_and_updates() {
        let (data, home, roots) = setup();
        let copy = crate::services::skills::duplicate(data.path(), "sagar-fiction").unwrap();
        install(data.path(), &roots, &copy.id, "claude_code", false).unwrap();
        let text = crate::services::skills::source(data.path(), &copy.id).unwrap() + "\nMore.";
        crate::services::skills::save(data.path(), &copy.id, &text).unwrap();
        assert_eq!(
            status(data.path(), &roots, &copy.id, "claude_code"),
            InstallStatus::UpdateAvailable
        );
        install(data.path(), &roots, &copy.id, "claude_code", false).unwrap();
        let installed = home
            .path()
            .join(".claude/skills")
            .join(&copy.id)
            .join("SKILL.md");
        assert_eq!(std::fs::read_to_string(installed).unwrap(), text);
        assert_eq!(
            status(data.path(), &roots, &copy.id, "claude_code"),
            InstallStatus::Installed
        );
    }

    #[test]
    fn changes_made_outside_sagar_need_force() {
        let (data, home, roots) = setup();
        install(data.path(), &roots, "sagar-xianxia", "codex", false).unwrap();
        let dir = home.path().join(".agents/skills/sagar-xianxia");
        std::fs::write(dir.join("SKILL.md"), "my own edits").unwrap();
        assert_eq!(
            status(data.path(), &roots, "sagar-xianxia", "codex"),
            InstallStatus::Modified
        );
        let refused = uninstall(data.path(), &roots, "sagar-xianxia", "codex", false).unwrap_err();
        assert_eq!(refused.code, "skill_install_modified");
        assert_eq!(
            install(data.path(), &roots, "sagar-xianxia", "codex", false)
                .unwrap_err()
                .code,
            "skill_install_modified"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
            "my own edits"
        );
        // An extra file added by the user also counts as a change.
        install(data.path(), &roots, "sagar-xianxia", "codex", true).unwrap();
        std::fs::write(dir.join("notes.md"), "mine").unwrap();
        assert_eq!(
            status(data.path(), &roots, "sagar-xianxia", "codex"),
            InstallStatus::Modified
        );
        uninstall(data.path(), &roots, "sagar-xianxia", "codex", true).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn folders_not_installed_by_sagar_are_never_touched() {
        let (data, home, roots) = setup();
        let dir = home.path().join(".gemini/config/skills/sagar-danmei");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), "someone else's").unwrap();
        assert_eq!(
            status(data.path(), &roots, "sagar-danmei", "agy"),
            InstallStatus::Conflict
        );
        for force in [false, true] {
            assert_eq!(
                install(data.path(), &roots, "sagar-danmei", "agy", force)
                    .unwrap_err()
                    .code,
                "skill_install_conflict"
            );
            assert_eq!(
                uninstall(data.path(), &roots, "sagar-danmei", "agy", force)
                    .unwrap_err()
                    .code,
                "skill_install_conflict"
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
            "someone else's"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_skill_folders_are_not_followed() {
        let (data, home, roots) = setup();
        let elsewhere = tempfile::tempdir().unwrap();
        let root = home.path().join(".qoder/skills");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.join("sagar-fiction")).unwrap();
        assert_eq!(
            status(data.path(), &roots, "sagar-fiction", "qoder"),
            InstallStatus::Conflict
        );
        assert!(install(data.path(), &roots, "sagar-fiction", "qoder", true).is_err());
        assert!(uninstall(data.path(), &roots, "sagar-fiction", "qoder", true).is_err());
        assert!(elsewhere.path().exists());
    }

    #[test]
    fn uninstall_everywhere_keeps_changed_copies() {
        let (data, home, roots) = setup();
        let copy = crate::services::skills::duplicate(data.path(), "sagar-fiction").unwrap();
        for cli in ["codex", "qoder"] {
            install(data.path(), &roots, &copy.id, cli, false).unwrap();
        }
        let changed = home
            .path()
            .join(".qoder/skills")
            .join(&copy.id)
            .join("SKILL.md");
        std::fs::write(&changed, "edited").unwrap();
        crate::services::skills::delete(data.path(), &copy.id).unwrap();
        assert_eq!(
            uninstall_everywhere(data.path(), &roots, &copy.id),
            vec!["Qoder"]
        );
        assert!(!home.path().join(".agents/skills").join(&copy.id).exists());
        assert!(changed.exists());
    }

    #[test]
    fn rejects_unknown_providers_and_skills() {
        let (data, _home, roots) = setup();
        assert!(install(data.path(), &roots, "sagar-fiction", "vim", false).is_err());
        assert!(install(data.path(), &roots, "missing", "codex", false).is_err());
        assert!(uninstall(data.path(), &roots, "../x", "codex", false).is_err());
        assert!(list(data.path(), &roots, &[], "missing").is_err());
    }
}
