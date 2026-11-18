//! Translation skills: short `SKILL.md` rule sets (standard Agent Skills
//! frontmatter plus a markdown body) that a book can select in Book Settings.
//! Sagar sends the selected skill bodies as guidance with every AI request, so
//! skills work the same for every CLI and custom API provider.
use crate::error::AppError;
use std::path::{Path, PathBuf};

/// Largest skill body, in bytes, sent with a request. Selected paragraphs share
/// a 24 KiB prompt budget, so skills stay well below it.
pub const MAX_BODY_BYTES: usize = 6 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillKind {
    Genre,
    Language,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub id: String,
    pub label: String,
    pub description: String,
    pub kind: SkillKind,
    /// Target language (primary subtag) for language skills.
    pub target: Option<String>,
    pub version: String,
    pub builtin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    #[serde(flatten)]
    pub summary: SkillSummary,
    pub body: String,
}

macro_rules! builtin {
    ($($id:literal),* $(,)?) => {
        &[$(($id, include_str!(concat!("../../skills/", $id, "/SKILL.md")))),*]
    };
}

/// Built-in skills, in the order they are offered.
const BUILTIN: &[(&str, &str)] = builtin!(
    "sagar-fiction",
    "sagar-nonfiction",
    "sagar-biography",
    "sagar-craft-career",
    "sagar-academic",
    "sagar-translated-lit",
    "sagar-children-ya",
    "sagar-wuxia",
    "sagar-xianxia",
    "sagar-xuanhuan",
    "sagar-danmei",
    "sagar-target-my",
);

fn invalid(message: impl Into<String>) -> AppError {
    AppError::new("invalid_skill", message)
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// True for a well-formed skill name (safe to use as a folder name).
pub fn is_valid_id(id: &str) -> bool {
    valid_id(id)
}

fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
}

/// Parses a `SKILL.md` file. Supports the flat `key: value` frontmatter used by
/// Agent Skills plus one nested `metadata:` map with Sagar's fields.
pub fn parse(text: &str, builtin: bool) -> Result<Skill, AppError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or_else(|| invalid("The skill must start with a frontmatter block (---)."))?;
    let end = rest
        .find("\n---")
        .ok_or_else(|| invalid("The skill's frontmatter block is not closed (---)."))?;
    let front = &rest[..end];
    let body = rest[end + 4..]
        .trim_start_matches(['\r', '\n'])
        .trim_end()
        .to_string();

    let (mut name, mut description) = (None, None);
    let (mut kind, mut label, mut target, mut version) = (None, None, None, None);
    let mut in_metadata = false;
    for line in front.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let nested = line.starts_with(' ') || line.starts_with('\t');
        let Some((key, value)) = line.trim().split_once(':') else {
            return Err(invalid("The skill's frontmatter has a line without a key."));
        };
        // Comments are only stripped from Sagar's own metadata values, so a
        // description may still contain " #".
        let value = if nested {
            value.split(" #").next().unwrap_or(value)
        } else {
            value
        };
        let value = unquote(value).to_string();
        if !nested {
            in_metadata = key == "metadata" && value.is_empty();
            match key {
                "name" => name = Some(value),
                "description" => description = Some(value),
                _ => {}
            }
        } else if in_metadata {
            match key {
                "sagar-kind" => kind = Some(value),
                "sagar-label" => label = Some(value),
                "sagar-target" => target = Some(value),
                "sagar-version" => version = Some(value),
                _ => {}
            }
        }
    }

    let id = name.ok_or_else(|| invalid("The skill needs a name."))?;
    if !valid_id(&id) {
        return Err(invalid(
            "The skill name must use lowercase letters, digits and single dashes (up to 64 characters).",
        ));
    }
    let description = description
        .filter(|d| !d.trim().is_empty())
        .ok_or_else(|| invalid("The skill needs a description."))?;
    if description.chars().count() > 1024 {
        return Err(invalid(
            "The skill description is longer than 1,024 characters.",
        ));
    }
    let kind = match kind.as_deref() {
        Some("genre") => SkillKind::Genre,
        Some("language") => SkillKind::Language,
        _ => {
            return Err(invalid(
                "The skill needs metadata.sagar-kind set to genre or language.",
            ));
        }
    };
    let target = target
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty());
    if kind == SkillKind::Language && target.is_none() {
        return Err(invalid("A language skill needs metadata.sagar-target."));
    }
    if body.is_empty() {
        return Err(invalid("The skill has no rules after its frontmatter."));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(invalid(format!(
            "The skill rules are too long (maximum {} KB).",
            MAX_BODY_BYTES / 1024
        )));
    }
    if body
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(invalid("The skill contains unsupported characters."));
    }
    Ok(Skill {
        summary: SkillSummary {
            label: label
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| id.clone()),
            id,
            description,
            kind,
            target: if kind == SkillKind::Language {
                target
            } else {
                None
            },
            version: version.unwrap_or_else(|| "1".into()),
            builtin,
        },
        body,
    })
}

fn builtin_skills() -> impl Iterator<Item = Skill> {
    // Built-in files are checked by tests; a broken one is skipped, not fatal.
    BUILTIN
        .iter()
        .filter_map(|(id, text)| parse(text, true).ok().filter(|s| s.summary.id == *id))
}

/// Largest `SKILL.md` file Sagar reads or imports.
const MAX_FILE_BYTES: u64 = 64 * 1024;
/// Prefix reserved for built-in skills, so app updates never clash with yours.
const RESERVED_PREFIX: &str = "sagar-";
const SKILL_FILE: &str = "SKILL.md";

fn user_root(data_dir: &Path) -> PathBuf {
    data_dir.join("skills")
}

fn not_found() -> AppError {
    AppError::new(
        "skill_not_found",
        "This translation skill is no longer available.",
    )
}

fn read_limited(path: &Path) -> Result<String, AppError> {
    let meta = std::fs::metadata(path)
        .map_err(|_| AppError::new("skill_unreadable", "The skill file could not be read."))?;
    if !meta.is_file() {
        return Err(invalid("Choose a SKILL.md file."));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(invalid("The skill file is too large (maximum 64 KB)."));
    }
    let bytes = std::fs::read(path)
        .map_err(|_| AppError::new("skill_unreadable", "The skill file could not be read."))?;
    String::from_utf8(bytes).map_err(|_| invalid("The skill file must be UTF-8 text."))
}

/// Reads one imported skill folder. Folders that are symlinks, have an
/// unexpected name or a broken `SKILL.md` are ignored.
fn read_user_skill(dir: &Path) -> Option<(Skill, String)> {
    let name = dir.file_name()?.to_str()?;
    if !valid_id(name) || name.starts_with(RESERVED_PREFIX) {
        return None;
    }
    if std::fs::symlink_metadata(dir)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return None;
    }
    let file = dir.join(SKILL_FILE);
    if std::fs::symlink_metadata(&file)
        .ok()?
        .file_type()
        .is_symlink()
    {
        return None;
    }
    let text = read_limited(&file).ok()?;
    let skill = parse(&text, false).ok()?;
    (skill.summary.id == name).then_some((skill, text))
}

fn user_skills(data_dir: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(user_root(data_dir)) else {
        return Vec::new();
    };
    let mut skills: Vec<Skill> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| read_user_skill(&entry.path()).map(|(skill, _)| skill))
        .collect();
    skills.sort_by(|a, b| {
        a.summary
            .label
            .to_lowercase()
            .cmp(&b.summary.label.to_lowercase())
    });
    skills
}

/// Built-in skills first, in catalog order, then imported skills by label.
pub fn list(data_dir: &Path) -> Vec<SkillSummary> {
    builtin_skills()
        .chain(user_skills(data_dir))
        .map(|s| s.summary)
        .collect()
}

pub fn get(data_dir: &Path, id: &str) -> Option<Skill> {
    if let Some(skill) = builtin_skills().find(|s| s.summary.id == id) {
        return Some(skill);
    }
    if !valid_id(id) {
        return None;
    }
    read_user_skill(&user_root(data_dir).join(id)).map(|(skill, _)| skill)
}

/// The complete `SKILL.md` text of a skill, for viewing or editing.
pub fn source(data_dir: &Path, id: &str) -> Result<String, AppError> {
    if let Some((_, text)) = BUILTIN.iter().find(|(builtin, _)| *builtin == id) {
        return Ok((*text).to_string());
    }
    if !valid_id(id) {
        return Err(not_found());
    }
    read_user_skill(&user_root(data_dir).join(id))
        .map(|(_, text)| text)
        .ok_or_else(not_found)
}

fn check_user_id(id: &str) -> Result<(), AppError> {
    if id.starts_with(RESERVED_PREFIX) {
        return Err(invalid(
            "Skill names starting with \"sagar-\" are reserved for built-in skills. Rename the skill in its frontmatter.",
        ));
    }
    Ok(())
}

/// Writes `SKILL.md` through a temporary file so an interrupted write never
/// leaves a half-written skill.
fn write_skill(data_dir: &Path, id: &str, text: &str) -> Result<(), AppError> {
    let dir = user_root(data_dir).join(id);
    if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(invalid("This skill folder can't be changed by Sagar."));
    }
    std::fs::create_dir_all(&dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
    std::io::Write::write_all(&mut temp, text.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(dir.join(SKILL_FILE))
        .map_err(|e| AppError::from(e.error))?;
    Ok(())
}

fn exists(data_dir: &Path, id: &str) -> bool {
    get(data_dir, id).is_some() || user_root(data_dir).join(id).exists()
}

/// Imports a `SKILL.md` file chosen by the user. Only that file is copied.
/// An imported skill with the same name is replaced only when `replace` is set.
pub fn import(data_dir: &Path, path: &Path, replace: bool) -> Result<SkillSummary, AppError> {
    let text = read_limited(path)?;
    let skill = parse(&text, false)?;
    let id = skill.summary.id.clone();
    check_user_id(&id)?;
    if exists(data_dir, &id) && !replace {
        return Err(AppError::with_details(
            "skill_exists",
            "A skill with this name already exists.",
            serde_json::json!({ "id": id, "label": skill.summary.label }),
        ));
    }
    write_skill(data_dir, &id, &text)?;
    Ok(skill.summary)
}

/// Saves edited `SKILL.md` text for an imported skill. The name can't change.
pub fn save(data_dir: &Path, id: &str, text: &str) -> Result<SkillSummary, AppError> {
    if BUILTIN.iter().any(|(builtin, _)| *builtin == id) {
        return Err(invalid(
            "Built-in skills can't be edited. Duplicate the skill to make an editable copy.",
        ));
    }
    if get(data_dir, id).is_none() {
        return Err(not_found());
    }
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid("The skill file is too large (maximum 64 KB)."));
    }
    let skill = parse(text, false)?;
    if skill.summary.id != id {
        return Err(invalid(format!(
            "Keep the skill name as \"{id}\". To use another name, duplicate the skill."
        )));
    }
    write_skill(data_dir, id, text)?;
    Ok(skill.summary)
}

fn render(summary: &SkillSummary, body: &str) -> String {
    let kind = match summary.kind {
        SkillKind::Genre => "genre",
        SkillKind::Language => "language",
    };
    let mut text = format!(
        "---\nname: {}\ndescription: {}\nmetadata:\n  sagar-kind: {kind}\n  sagar-label: {}\n",
        summary.id, summary.description, summary.label
    );
    if let Some(target) = &summary.target {
        text.push_str(&format!("  sagar-target: {target}\n"));
    }
    text.push_str(&format!(
        "  sagar-version: \"{}\"\n---\n{body}\n",
        summary.version
    ));
    text
}

/// Makes an editable copy of any skill under a new, free name.
pub fn duplicate(data_dir: &Path, id: &str) -> Result<SkillSummary, AppError> {
    let skill = get(data_dir, id).ok_or_else(not_found)?;
    let base = id.strip_prefix(RESERVED_PREFIX).unwrap_or(id);
    let base: String = base.chars().take(50).collect();
    let base = base.trim_end_matches('-');
    let new_id = (1..1000)
        .map(|n| {
            if n == 1 {
                format!("{base}-copy")
            } else {
                format!("{base}-copy-{n}")
            }
        })
        .find(|candidate| valid_id(candidate) && !exists(data_dir, candidate))
        .ok_or_else(|| invalid("A free name for the copy could not be found."))?;
    let summary = SkillSummary {
        id: new_id,
        label: format!("{} (copy)", skill.summary.label),
        builtin: false,
        ..skill.summary
    };
    let text = render(&summary, &skill.body);
    let parsed = parse(&text, false)?;
    write_skill(data_dir, &summary.id, &text)?;
    Ok(parsed.summary)
}

/// Number of books that use the skill.
pub fn usage(data_dir: &Path, id: &str) -> Result<usize, AppError> {
    let conn = crate::services::workspace::open_db(data_dir)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM projects WHERE translation_skill_id=?1 OR language_skill_id=?1",
        [id],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as usize)
}

/// Deletes an imported skill. Books that used it go back to no skill.
pub fn delete(data_dir: &Path, id: &str) -> Result<(), AppError> {
    if BUILTIN.iter().any(|(builtin, _)| *builtin == id) {
        return Err(invalid("Built-in skills can't be deleted."));
    }
    if !valid_id(id) || id.starts_with(RESERVED_PREFIX) {
        return Err(not_found());
    }
    let dir = user_root(data_dir).join(id);
    let meta = std::fs::symlink_metadata(&dir).map_err(|_| not_found())?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(invalid("This skill folder can't be changed by Sagar."));
    }
    let mut conn = crate::services::workspace::open_db(data_dir)?;
    let tx = crate::storage::begin_write(&mut conn)?;
    tx.execute(
        "UPDATE projects SET translation_skill_id=NULL WHERE translation_skill_id=?1",
        [id],
    )?;
    tx.execute(
        "UPDATE projects SET language_skill_id=NULL WHERE language_skill_id=?1",
        [id],
    )?;
    tx.commit()?;
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

fn primary_subtag(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// True when a language skill applies to the book's target language tag.
pub fn matches_target(skill: &SkillSummary, target_language: &str) -> bool {
    skill.kind != SkillKind::Language
        || skill.target.as_deref() == Some(primary_subtag(target_language).as_str())
}

/// Checks a Book Settings selection: the ID must exist and have the right kind.
pub fn validate_selection(
    data_dir: &Path,
    id: Option<&str>,
    kind: SkillKind,
    target_language: &str,
) -> Result<Option<String>, AppError> {
    let Some(id) = id.map(str::trim).filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let skill = get(data_dir, id).ok_or_else(|| {
        AppError::new(
            "skill_not_found",
            "The selected translation skill is no longer available. Choose another skill.",
        )
    })?;
    if skill.summary.kind != kind {
        return Err(AppError::new(
            "invalid_book_settings",
            "This skill can't be used for that choice.",
        ));
    }
    if !matches_target(&skill.summary, target_language) {
        return Err(AppError::new(
            "invalid_book_settings",
            "The language skill doesn't match the book's translation language.",
        ));
    }
    Ok(Some(skill.summary.id))
}

/// Renders the selected skills as guidance text for a translation request:
/// the language skill first, then the genre skill. Missing skills and a
/// language skill that no longer matches the target language are skipped.
pub fn guidance(
    data_dir: &Path,
    genre: Option<&str>,
    language: Option<&str>,
    target_language: &str,
) -> String {
    let language = language
        .and_then(|id| get(data_dir, id))
        .filter(|s| s.summary.kind == SkillKind::Language)
        .filter(|s| matches_target(&s.summary, target_language));
    let genre = genre
        .and_then(|id| get(data_dir, id))
        .filter(|s| s.summary.kind == SkillKind::Genre);
    let parts: Vec<String> = language.into_iter().chain(genre).map(|s| s.body).collect();
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> &'static Path {
        Path::new("/nonexistent-sagar-data")
    }

    fn user_skill(id: &str, kind: &str) -> String {
        format!(
            "---\nname: {id}\ndescription: My rules: keep names.\nmetadata:\n  sagar-kind: {kind}\n  sagar-label: Mine\n  sagar-target: my\n---\n# Rules\nKeep it short.\n"
        )
    }

    #[test]
    fn every_builtin_skill_parses_with_its_folder_id() {
        assert_eq!(list(none()).len(), BUILTIN.len());
        for (id, text) in BUILTIN {
            let skill = parse(text, true).unwrap_or_else(|e| panic!("{id}: {}", e.message));
            assert_eq!(skill.summary.id, *id);
            assert!(id.starts_with(RESERVED_PREFIX));
            assert!(skill.body.len() <= MAX_BODY_BYTES, "{id} is too long");
            assert!(!skill.body.starts_with("---"));
        }
        let my = get(none(), "sagar-target-my").unwrap();
        assert_eq!(my.summary.kind, SkillKind::Language);
        assert_eq!(my.summary.target.as_deref(), Some("my"));
        assert_eq!(
            get(none(), "sagar-xianxia").unwrap().summary.kind,
            SkillKind::Genre
        );
    }

    #[test]
    fn parse_rejects_bad_frontmatter() {
        let ok = "---\nname: a-skill\ndescription: d\nmetadata:\n  sagar-kind: genre\n---\nRules";
        assert!(parse(ok, false).is_ok());
        for bad in [
            "name: x\n",
            "---\nname: x\ndescription: d\nmetadata:\n  sagar-kind: genre\nRules",
            "---\nname: Bad Name\ndescription: d\nmetadata:\n  sagar-kind: genre\n---\nR",
            "---\nname: ../x\ndescription: d\nmetadata:\n  sagar-kind: genre\n---\nR",
            "---\nname: x\nmetadata:\n  sagar-kind: genre\n---\nR",
            "---\nname: x\ndescription: d\n---\nR",
            "---\nname: x\ndescription: d\nmetadata:\n  sagar-kind: language\n---\nR",
            "---\nname: x\ndescription: d\nmetadata:\n  sagar-kind: genre\n---\n",
        ] {
            assert!(parse(bad, false).is_err(), "{bad}");
        }
        let long = format!("{ok}{}", "x".repeat(MAX_BODY_BYTES));
        assert!(parse(&long, false).is_err());
    }

    #[test]
    fn parse_reads_quoted_and_crlf_values() {
        let text = "---\r\nname: \"my-skill\"\r\ndescription: 'A d # not a comment'\r\nmetadata:\r\n  sagar-kind: language # kind\r\n  sagar-target: MY\r\n  sagar-label: Mine\r\n---\r\n# Rules\r\n";
        let skill = parse(text, false).unwrap();
        assert_eq!(skill.summary.id, "my-skill");
        assert_eq!(skill.summary.description, "A d # not a comment");
        assert_eq!(skill.summary.label, "Mine");
        assert_eq!(skill.summary.target.as_deref(), Some("my"));
        assert_eq!(skill.body, "# Rules");
    }

    #[test]
    fn selection_checks_existence_kind_and_target_language() {
        let d = none();
        assert_eq!(
            validate_selection(d, None, SkillKind::Genre, "my").unwrap(),
            None
        );
        assert_eq!(
            validate_selection(d, Some(" "), SkillKind::Genre, "my").unwrap(),
            None
        );
        assert_eq!(
            validate_selection(d, Some("sagar-fiction"), SkillKind::Genre, "my").unwrap(),
            Some("sagar-fiction".into())
        );
        assert!(validate_selection(d, Some("missing"), SkillKind::Genre, "my").is_err());
        assert!(validate_selection(d, Some("sagar-fiction"), SkillKind::Language, "my").is_err());
        assert!(
            validate_selection(d, Some("sagar-target-my"), SkillKind::Language, "my-MM").is_ok()
        );
        assert!(validate_selection(d, Some("sagar-target-my"), SkillKind::Language, "th").is_err());
    }

    #[test]
    fn guidance_puts_language_first_and_skips_mismatches() {
        let d = none();
        let my = get(d, "sagar-target-my").unwrap().body;
        let xianxia = get(d, "sagar-xianxia").unwrap().body;
        let both = guidance(d, Some("sagar-xianxia"), Some("sagar-target-my"), "my");
        assert_eq!(both, format!("{my}\n\n{xianxia}"));
        assert_eq!(
            guidance(d, Some("sagar-xianxia"), Some("sagar-target-my"), "th"),
            xianxia
        );
        assert_eq!(guidance(d, Some("missing"), None, "my"), "");
        assert_eq!(
            guidance(d, Some("sagar-target-my"), Some("sagar-fiction"), "my"),
            ""
        );
    }

    #[test]
    fn import_lists_and_guides_with_an_imported_skill() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("picked.md");
        std::fs::write(&file, user_skill("house-style", "genre")).unwrap();
        let summary = import(data.path(), &file, false).unwrap();
        assert_eq!(summary.id, "house-style");
        assert!(!summary.builtin);
        assert!(data.path().join("skills/house-style/SKILL.md").is_file());
        let all = list(data.path());
        assert_eq!(all.len(), BUILTIN.len() + 1);
        assert_eq!(all.last().unwrap().id, "house-style");
        assert_eq!(
            source(data.path(), "house-style").unwrap(),
            user_skill("house-style", "genre")
        );
        assert_eq!(
            guidance(data.path(), Some("house-style"), None, "my"),
            "# Rules\nKeep it short."
        );
        assert!(
            validate_selection(data.path(), Some("house-style"), SkillKind::Genre, "my").is_ok()
        );
        // A second import with the same name asks before replacing.
        let again = import(data.path(), &file, false).unwrap_err();
        assert_eq!(again.code, "skill_exists");
        std::fs::write(
            &file,
            user_skill("house-style", "genre").replace("short", "brief"),
        )
        .unwrap();
        import(data.path(), &file, true).unwrap();
        assert!(
            get(data.path(), "house-style")
                .unwrap()
                .body
                .contains("brief")
        );
    }

    #[test]
    fn import_rejects_reserved_names_large_and_invalid_files() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("SKILL.md");
        std::fs::write(&file, user_skill("sagar-fiction", "genre")).unwrap();
        assert!(import(data.path(), &file, true).is_err());
        std::fs::write(&file, user_skill("sagar-new", "genre")).unwrap();
        assert!(import(data.path(), &file, true).is_err());
        std::fs::write(&file, "x".repeat(70 * 1024)).unwrap();
        assert!(import(data.path(), &file, false).is_err());
        std::fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();
        assert!(import(data.path(), &file, false).is_err());
        assert!(import(data.path(), data.path(), false).is_err());
        assert_eq!(list(data.path()).len(), BUILTIN.len());
    }

    #[test]
    fn broken_or_mismatched_folders_are_ignored() {
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("skills");
        std::fs::create_dir_all(root.join("broken")).unwrap();
        std::fs::write(root.join("broken/SKILL.md"), "not a skill").unwrap();
        std::fs::create_dir_all(root.join("other-name")).unwrap();
        std::fs::write(
            root.join("other-name/SKILL.md"),
            user_skill("real-name", "genre"),
        )
        .unwrap();
        assert_eq!(list(data.path()).len(), BUILTIN.len());
        assert!(get(data.path(), "other-name").is_none());
        assert!(get(data.path(), "../skills").is_none());
    }

    #[test]
    fn save_keeps_the_name_and_refuses_builtins() {
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("x.md");
        std::fs::write(&file, user_skill("mine", "language")).unwrap();
        import(data.path(), &file, false).unwrap();
        let edited = user_skill("mine", "language").replace("Keep it short.", "Use ။");
        let summary = save(data.path(), "mine", &edited).unwrap();
        assert_eq!(summary.target.as_deref(), Some("my"));
        assert_eq!(source(data.path(), "mine").unwrap(), edited);
        assert!(save(data.path(), "mine", &user_skill("renamed", "language")).is_err());
        assert!(save(data.path(), "mine", "broken").is_err());
        assert_eq!(source(data.path(), "mine").unwrap(), edited);
        assert!(save(data.path(), "sagar-fiction", BUILTIN[0].1).is_err());
        assert!(save(data.path(), "missing", &user_skill("missing", "genre")).is_err());
    }

    #[test]
    fn duplicate_makes_an_editable_copy_with_a_free_name() {
        let data = tempfile::tempdir().unwrap();
        let first = duplicate(data.path(), "sagar-xianxia").unwrap();
        assert_eq!(first.id, "xianxia-copy");
        assert_eq!(first.label, "Xianxia 仙侠 (copy)");
        assert_eq!(first.kind, SkillKind::Genre);
        assert!(!first.builtin);
        let second = duplicate(data.path(), "sagar-xianxia").unwrap();
        assert_eq!(second.id, "xianxia-copy-2");
        let language = duplicate(data.path(), "sagar-target-my").unwrap();
        assert_eq!(language.target.as_deref(), Some("my"));
        assert_eq!(
            get(data.path(), "xianxia-copy").unwrap().body,
            get(data.path(), "sagar-xianxia").unwrap().body
        );
        let edited = source(data.path(), "xianxia-copy").unwrap() + "\nExtra rule.";
        save(data.path(), "xianxia-copy", &edited).unwrap();
        assert!(duplicate(data.path(), "missing").is_err());
    }

    #[test]
    fn delete_clears_books_that_used_the_skill() {
        let data = tempfile::tempdir().unwrap();
        let copy = duplicate(data.path(), "sagar-fiction").unwrap();
        let conn = crate::services::workspace::open_db(data.path()).unwrap();
        for (id, genre) in [("a", copy.id.as_str()), ("b", "sagar-fiction")] {
            conn.execute(
                "INSERT INTO projects(id, source_sha256, source_snapshot_path, title, created_at, updated_at, translation_skill_id) VALUES(?1,'h','p','t','now','now',?2)",
                rusqlite::params![id, genre],
            )
            .unwrap();
        }
        assert_eq!(usage(data.path(), &copy.id).unwrap(), 1);
        assert!(delete(data.path(), "sagar-fiction").is_err());
        delete(data.path(), &copy.id).unwrap();
        assert!(get(data.path(), &copy.id).is_none());
        assert!(!data.path().join("skills").join(&copy.id).exists());
        let skills: Vec<Option<String>> = conn
            .prepare("SELECT translation_skill_id FROM projects ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(skills, vec![None, Some("sagar-fiction".to_string())]);
        assert!(delete(data.path(), &copy.id).is_err());
        assert!(delete(data.path(), "../x").is_err());
    }
}
