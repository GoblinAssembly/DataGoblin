use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Manager, State};

#[derive(Default)]
pub struct LibraryLock(Mutex<()>);

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedQuery {
    pub id: String,
    pub name: String,
    pub connection_id: String,
    pub server_name: String,
    pub database: String,
    pub sql: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    folder: String,
    default_folder: String,
    queries: Vec<SavedQuery>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Preferences {
    query_folder: Option<PathBuf>,
}

fn fail(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn default_folder(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(fail)?.join("queries"))
}
fn preferences_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let path = app.path().app_config_dir().map_err(fail)?;
    fs::create_dir_all(&path).map_err(fail)?;
    Ok(path.join("preferences.json"))
}
fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let path = preferences_path(app)?;
    let prefs: Preferences = if path.exists() {
        serde_json::from_slice(&fs::read(path).map_err(fail)?).map_err(fail)?
    } else {
        Preferences::default()
    };
    prepare_root(&prefs.query_folder.unwrap_or(default_folder(app)?))
}
fn prepare_root(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Choose an absolute folder path.".into());
    }
    fs::create_dir_all(path).map_err(fail)?;
    fs::canonicalize(path).map_err(fail)
}
fn slug(name: &str) -> String {
    let text: String = name
        .chars()
        .take(48)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if text.is_empty() {
        "untitled".into()
    } else {
        text
    }
}
fn category(name: &str) -> String {
    let hash = Sha256::digest(name.as_bytes());
    format!("{}-{:x}", slug(name), hash)[..]
        .chars()
        .take(slug(name).chars().count() + 17)
        .collect()
}
fn relative_path(query: &SavedQuery) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(&query.id).map_err(|_| "Invalid query ID".to_string())?;
    if query.draft {
        return Ok(PathBuf::from("Unsaved").join(format!(
            "{}-{}.sql",
            slug(&query.name),
            query.id
        )));
    }
    let server = if query.connection_id.is_empty() {
        "Unassigned".into()
    } else {
        uuid::Uuid::parse_str(&query.connection_id)
            .map_err(|_| "Invalid connection ID".to_string())?;
        format!("{}-{}", slug(&query.server_name), query.connection_id)
    };
    Ok(PathBuf::from(server)
        .join(category(if query.database.is_empty() {
            "No database"
        } else {
            &query.database
        }))
        .join(format!(
            "{}-{}.sql",
            slug(query.name.trim_end_matches(".sql")),
            query.id
        )))
}
fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut path = root.to_owned();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("Invalid library path".into());
        }
        path.push(component);
        if path.exists()
            && fs::symlink_metadata(&path)
                .map_err(fail)?
                .file_type()
                .is_symlink()
        {
            return Err("Query library paths cannot contain symbolic links.".into());
        }
        if path.exists() && !fs::canonicalize(&path).map_err(fail)?.starts_with(root) {
            return Err("Query path is outside the configured folder.".into());
        }
    }
    Ok(path)
}
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing parent directory")?;
    fs::create_dir_all(parent).map_err(fail)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(fail)?;
    temp.write_all(data).map_err(fail)?;
    temp.as_file().sync_all().map_err(fail)?;
    temp.persist(path).map_err(fail)?;
    Ok(())
}
fn metadata_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(fail)?;
    safe_path(
        root,
        &PathBuf::from(".datagoblin").join(format!("{id}.json")),
    )
}
fn store_query(root: &Path, query: &SavedQuery) -> Result<(), String> {
    let path = safe_path(root, &relative_path(query)?)?;
    let metadata = metadata_path(root, &query.id)?;
    let previous: Option<SavedQuery> = if metadata.exists() {
        Some(serde_json::from_slice(&fs::read(&metadata).map_err(fail)?).map_err(fail)?)
    } else {
        None
    };
    atomic_write(&path, query.sql.as_bytes())?;
    atomic_write(&metadata, &serde_json::to_vec_pretty(query).map_err(fail)?)?;
    if let Some(previous) = previous {
        let previous_path = safe_path(root, &relative_path(&previous)?)?;
        if previous_path != path && previous_path.exists()
            && !same_file::is_same_file(&previous_path, &path).map_err(fail)?
        {
            fs::remove_file(previous_path).map_err(fail)?;
        }
    }
    Ok(())
}
fn delete_query(root: &Path, id: &str) -> Result<(), String> {
    let metadata = metadata_path(root, id)?;
    if !metadata.exists() {
        return Ok(());
    }
    let bytes = fs::read(&metadata).map_err(fail)?;
    let query: SavedQuery = serde_json::from_slice(&bytes).map_err(fail)?;
    if query.id != id {
        return Err("Query metadata ID mismatch.".into());
    }
    let sql_path = safe_path(root, &relative_path(&query)?)?;
    let sql = fs::read(&sql_path).map_err(fail)?;
    // Keep a recovery copy outside the visible library before removing live files.
    let archive = PathBuf::from(".datagoblin").join("trash");
    atomic_write(&safe_path(root, &archive.join(format!("{id}.sql")))?, &sql)?;
    atomic_write(
        &safe_path(root, &archive.join(format!("{id}.json")))?,
        &bytes,
    )?;
    fs::remove_file(&sql_path).map_err(fail)?;
    if let Err(error) = fs::remove_file(&metadata) {
        atomic_write(&sql_path, &sql)?;
        return Err(fail(error));
    }
    Ok(())
}

fn read_queries(root: &Path) -> Result<Vec<SavedQuery>, String> {
    let index = safe_path(root, Path::new(".datagoblin"))?;
    if !index.exists() {
        return Ok(vec![]);
    }
    let mut queries = Vec::new();
    for file in fs::read_dir(index).map_err(fail)? {
        let file = file.map_err(fail)?;
        if file.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let mut query: SavedQuery =
            serde_json::from_slice(&fs::read(file.path()).map_err(fail)?).map_err(fail)?;
        let path = safe_path(root, &relative_path(&query)?)?;
        query.sql = fs::read_to_string(path).map_err(fail)?;
        queries.push(query);
    }
    queries.sort_by(|a, b| {
        (&a.server_name, &a.database, &a.name).cmp(&(&b.server_name, &b.database, &b.name))
    });
    Ok(queries)
}
fn copy_queries(source: &Path, destination: &Path) -> Result<(), String> {
    if source == destination {
        return Ok(());
    }
    let queries = read_queries(source)?;
    let existing = read_queries(destination)?;
    // Validate the whole set before writing, so switching never overwrites a different query.
    for query in &queries {
        if existing
            .iter()
            .any(|entry| entry.id == query.id && entry != query)
        {
            return Err(format!("The destination already contains a different version of '{}'. Choose another folder.", query.name));
        }
        let path = safe_path(destination, &relative_path(query)?)?;
        if path.exists() && fs::read_to_string(&path).map_err(fail)? != query.sql {
            return Err(format!(
                "A different SQL file already exists: {}",
                path.display()
            ));
        }
    }
    for query in queries {
        store_query(destination, &query)?;
    }
    Ok(())
}
fn snapshot(app: &tauri::AppHandle) -> Result<Library, String> {
    let root = root(app)?;
    fs::create_dir_all(safe_path(&root, Path::new("Unsaved"))?).map_err(fail)?;
    Ok(Library {
        folder: root.display().to_string(),
        default_folder: default_folder(app)?.display().to_string(),
        queries: read_queries(&root)?,
    })
}
#[tauri::command]
pub fn query_library(
    app: tauri::AppHandle,
    state: State<'_, LibraryLock>,
) -> Result<Library, String> {
    let _guard = state.0.lock().map_err(fail)?;
    snapshot(&app)
}
#[tauri::command]
pub fn save_library_query(
    app: tauri::AppHandle,
    state: State<'_, LibraryLock>,
    query: SavedQuery,
) -> Result<(), String> {
    let _guard = state.0.lock().map_err(fail)?;
    store_query(&root(&app)?, &query)
}
#[tauri::command]
pub fn delete_library_query(
    app: tauri::AppHandle,
    state: State<'_, LibraryLock>,
    id: String,
) -> Result<(), String> {
    let _guard = state.0.lock().map_err(fail)?;
    delete_query(&root(&app)?, &id)
}

#[tauri::command]
pub fn set_query_folder(
    app: tauri::AppHandle,
    state: State<'_, LibraryLock>,
    folder: Option<String>,
) -> Result<Library, String> {
    let _guard = state.0.lock().map_err(fail)?;
    let source = root(&app)?;
    let destination = prepare_root(
        &folder
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or(default_folder(&app)?),
    )?;
    copy_queries(&source, &destination)?;
    // Probe write permission even for an empty library.
    let probe = tempfile::NamedTempFile::new_in(&destination).map_err(fail)?;
    drop(probe);
    atomic_write(
        &preferences_path(&app)?,
        &serde_json::to_vec_pretty(&Preferences {
            query_folder: folder.map(|_| destination),
        })
        .map_err(fail)?,
    )?;
    snapshot(&app)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> SavedQuery {
        SavedQuery {
            id: "4eb5ecf4-3dda-4413-8780-8a4338525cbd".into(),
            name: "../Revenue:report".into(),
            connection_id: "76e393fc-06b8-4e29-ae92-fb196b517aab".into(),
            server_name: "Local/server".into(),
            database: "Sales/2026".into(),
            sql: "SELECT 42;".into(),
            draft: false,
            pinned: false,
        }
    }
    #[test]
    fn saves_and_reloads_sql_with_context_and_moves_on_rename() {
        let dir = tempfile::tempdir().unwrap();
        let root = prepare_root(dir.path()).unwrap();
        let mut query = sample();
        store_query(&root, &query).unwrap();
        let old = root.join(relative_path(&query).unwrap());
        assert!(old.starts_with(&root));
        assert_eq!(read_queries(&root).unwrap(), vec![query.clone()]);
        query.name = "New name".into();
        query.database = "Other".into();
        query.sql = "SELECT 43;".into();
        store_query(&root, &query).unwrap();
        assert!(!old.exists());
        assert_eq!(read_queries(&root).unwrap(), vec![query]);
    }
    #[test]
    fn case_only_rename_preserves_saved_sql() {
        let dir = tempfile::tempdir().unwrap();
        let root = prepare_root(dir.path()).unwrap();
        let mut query = sample();
        query.name = "revenue".into();
        store_query(&root, &query).unwrap();
        query.name = "Revenue".into();
        query.sql = "SELECT 43;".into();
        store_query(&root, &query).unwrap();
        assert_eq!(read_queries(&root).unwrap(), vec![query]);
    }
    #[test]
    fn migration_preserves_originals_and_refuses_conflicting_versions() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let a = prepare_root(a.path()).unwrap();
        let b = prepare_root(b.path()).unwrap();
        let query = sample();
        store_query(&a, &query).unwrap();
        copy_queries(&a, &b).unwrap();
        assert_eq!(read_queries(&a).unwrap(), read_queries(&b).unwrap());
        let mut changed = query.clone();
        changed.sql = "different".into();
        store_query(&b, &changed).unwrap();
        assert!(copy_queries(&a, &b).is_err());
        assert_eq!(read_queries(&b).unwrap(), vec![changed]);
    }
    #[test]
    fn names_cannot_escape_root_or_collide_after_sanitization() {
        let mut query = sample();
        query.id = "../bad".into();
        assert!(relative_path(&query).is_err());
        assert_ne!(category("a/b"), category("a:b"));
        assert_ne!(category("Sales"), category("sales"));
    }
    #[test]
    fn pin_survives_reload_and_delete_removes_only_selected_query() {
        let dir = tempfile::tempdir().unwrap();
        let root = prepare_root(dir.path()).unwrap();
        let mut query = sample();
        query.pinned = true;
        store_query(&root, &query).unwrap();
        let mut other = query.clone();
        other.id = "56a339c7-719c-4c89-bd73-73d9c090ff15".into();
        store_query(&root, &other).unwrap();
        assert!(read_queries(&root).unwrap().iter().all(|q| q.pinned));
        delete_query(&root, &query.id).unwrap();
        assert_eq!(read_queries(&root).unwrap(), vec![other]);
        assert!(!root.join(relative_path(&query).unwrap()).exists());
        assert_eq!(
            fs::read_to_string(
                root.join(".datagoblin/trash")
                    .join(format!("{}.sql", query.id))
            )
            .unwrap(),
            query.sql
        );
        assert!(delete_query(&root, "../bad").is_err());
    }
    #[test]
    fn drafts_follow_the_library_root_and_promote_without_losing_context() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let source = prepare_root(source.path()).unwrap();
        let destination = prepare_root(destination.path()).unwrap();
        let mut query = sample();
        query.name = "Query3".into();
        query.draft = true;
        store_query(&source, &query).unwrap();
        let draft_path = relative_path(&query).unwrap();
        assert_eq!(draft_path.parent().unwrap(), Path::new("Unsaved"));
        copy_queries(&source, &destination).unwrap();
        assert!(destination.join(&draft_path).exists());
        assert_eq!(read_queries(&destination).unwrap(), vec![query.clone()]);
        query.name = "Revenue".into();
        query.draft = false;
        store_query(&destination, &query).unwrap();
        assert!(!destination.join(draft_path).exists());
        assert!(destination.join(relative_path(&query).unwrap()).exists());
        assert_eq!(read_queries(&destination).unwrap(), vec![query]);
    }
}
