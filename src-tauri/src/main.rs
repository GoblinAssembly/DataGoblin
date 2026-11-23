#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod column_keys;
mod count_query;
mod database;
mod foreign_keys;
mod history;
mod library;
#[cfg(test)]
mod live_perf;
mod result_cache;
mod result_session;
mod table_browser;
mod table_edit;
mod join_edit;
use database::{Connection, QueryResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};
use tokio::sync::oneshot;

#[derive(Default)]
struct Runtime {
    edited_results: Mutex<std::collections::HashSet<String>>,
    running: Mutex<HashMap<String, oneshot::Sender<()>>>,
    results: Mutex<HashMap<String, (std::time::Instant, Arc<StoredResult>)>>,
}

struct StoredResult {
    join: Option<join_edit::JoinEdit>,
    join_error: Option<String>,
    session: result_session::Session,
    connection: Connection,
    password: String,
    count_sql: Option<String>,
    base_count_sql: Option<String>,
    browser: Option<table_browser::Browser>,
    parameters: Option<Vec<String>>,
}
impl StoredResult {
    fn decorate(&self, result: &mut QueryResult) {
        if let Some(set) = result.sets.first_mut() {
            if let Some(join) = &self.join { join.decorate(set); }
            else if self.browser.is_none() { set.edit_reason = Some(self.join_error.clone().unwrap_or_else(|| "Editing supports plain tables or SELECT * with INNER JOINs across multiple tables. All source tables need primary keys; expressions, outer joins and self-joins are read-only.".into())); }
        }
    }
}
impl std::ops::Deref for StoredResult {
    type Target = result_session::Session;
    fn deref(&self) -> &Self::Target {
        &self.session
    }
}

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    connections: Vec<Connection>,
}

fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("connections.json"))
}

#[tauri::command]
fn load_connections(app: tauri::AppHandle) -> Result<Vec<Connection>, String> {
    let path = settings_path(&app)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let settings: Settings = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(settings.connections)
}

#[tauri::command]
fn save_connections(app: tauri::AppHandle, connections: Vec<Connection>) -> Result<(), String> {
    let data =
        serde_json::to_string_pretty(&Settings { connections }).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(&app)?, data).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_password(id: String, password: String) -> Result<(), String> {
    keyring::Entry::new("DataGoblin", &id)
        .map_err(|e| e.to_string())?
        .set_password(&password)
        .map_err(|e| format!("Could not save password in your system credential store: {e}"))
}

#[tauri::command]
fn forget_password(id: String) -> Result<(), String> {
    match keyring::Entry::new("DataGoblin", &id)
        .map_err(|e| e.to_string())?
        .delete_credential()
    {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn password_for(c: &Connection, password: Option<String>) -> Result<String, String> {
    if let Some(password) = password {
        return Ok(password);
    }
    keyring::Entry::new("DataGoblin", &c.id)
        .map_err(|e| e.to_string())?
        .get_password()
        .map_err(|_| "No saved password found. Edit the connection and enter a password.".into())
}

#[tauri::command]
async fn query(
    state: State<'_, Runtime>,
    request_id: String,
    connection: Connection,
    password: Option<String>,
    sql: String,
    paged: Option<bool>,
    page_size: Option<usize>,
) -> Result<QueryResult, String> {
    let password = password_for(&connection, password)?;
    let (tx, rx) = oneshot::channel();
    {
        let mut running = state.running.lock().map_err(|e| e.to_string())?;
        if running.contains_key(&request_id) {
            return Err("Duplicate request ID".into());
        }
        running.insert(request_id.clone(), tx);
    }
    let start = std::time::Instant::now();
    let fetch = async {
        if paged.unwrap_or(false) {
            let size = page_size.unwrap_or(500);
            if size == 0 || size > 10000 {
                return Err("Initial page size must be between 1 and 10,000.".into());
            }
            let count_connection = connection.clone();
            let count_password = password.clone();
            let count_sql = count_query::table_count(&sql, &connection.engine);
            let (join, join_error) = if count_sql.is_none() {
                match tokio::time::timeout(std::time::Duration::from_secs(10), join_edit::prepare(&connection, &password, &sql)).await {
                    Ok(Ok(join)) => (join,None),
                    Ok(Err(error)) => (None,Some(error)),
                    Err(_) => (None,Some("JOIN editing metadata timed out. Results are read-only.".into())),
                }
            } else { (None,None) };
            let session = Arc::new(StoredResult {
                join, join_error,
                connection: count_connection,
                password: count_password,
                base_count_sql: count_query::table_query(&sql, &connection.engine).map(|q| q.base_count),
                browser: table_browser::initial(&sql, &connection.engine),
                parameters: None,
                count_sql,
                session: result_session::start(move |events| {
                    database::stream(connection, password, sql, events)
                })?,
            });
            let mut result = session.first_page(size).await?.result;
            session.decorate(&mut result);
            result.browser = session.browser.clone();
            result.cache_id = Some(request_id.clone());
            result.elapsed_ms = start.elapsed().as_millis();
            let mut results = state.results.lock().map_err(|e| e.to_string())?;
            if results.len() >= 4 {
                if let Some(oldest) = results
                    .iter()
                    .min_by_key(|(_, (time, _))| *time)
                    .map(|(key, _)| key.clone())
                {
                    if let Some((_, session)) = results.remove(&oldest) {
                        session.cancel();
                    }
                }
            }
            results.insert(request_id.clone(), (std::time::Instant::now(), session));
            Ok(result)
        } else {
            database::execute(&connection, &password, &sql).await
        }
    };
    let result = tokio::select! {
        result = fetch => result,
        _ = rx => Err("Query cancelled. The connection was closed; check server state before retrying writes.".into()),
        _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => Err("Query timed out after 120 seconds. The connection was closed.".into()),
    };
    state
        .running
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&request_id);
    result
}

#[tauri::command]
async fn result_page(
    state: State<'_, Runtime>,
    cache_id: String,
    set_index: usize,
    offset: usize,
    page_size: usize,
) -> Result<result_session::Page, String> {
    let session = {
        let mut results = state.results.lock().map_err(|e| e.to_string())?;
        let (accessed, session) = results
            .get_mut(&cache_id)
            .ok_or("Results have expired. Run the query again to load them.")?;
        *accessed = std::time::Instant::now();
        session.clone()
    };
    let mut page = session.page(set_index, offset, page_size).await?;
    session.decorate(&mut page.result);
    page.result.browser = session.browser.clone();
    page.result.cache_id = Some(cache_id);
    Ok(page)
}

#[tauri::command]
async fn refresh_join_results(state: State<'_, Runtime>, cache_id: String, request_id: String, page_size: Option<usize>) -> Result<QueryResult,String> {
    let old=stored_result(&state,&cache_id)?;
    let sql=old.join.as_ref().ok_or("Only supported JOIN results can be refreshed here.")?.sql.clone();
    // Rerun the exact SELECT and connection that produced this snapshot, not the editor's current text.
    let result=query(state.clone(),request_id,old.connection.clone(),Some(old.password.clone()),sql,Some(true),page_size).await?;
    if let Some((_,previous))=state.results.lock().map_err(|e|e.to_string())?.remove(&cache_id) {previous.cancel();}
    state.edited_results.lock().map_err(|e|e.to_string())?.remove(&cache_id);
    Ok(result)
}

#[tauri::command]
async fn edit_capabilities(state: State<'_, Runtime>, cache_id: String) -> Result<table_edit::Capabilities, String> {
    if state.edited_results.lock().map_err(|e| e.to_string())?.contains(&cache_id) { return Err("Reload results before editing again.".into()); }
    let stored = stored_result(&state, &cache_id)?;
    if let Some(join) = &stored.join {
        let page=stored.page(0,0,1).await?;
        join.validate(&page.result.sets.first().ok_or("Missing result set")?.columns)?;
        return Ok(join.capabilities());
    }
    let base = stored.base_count_sql.as_deref().filter(|_| stored.browser.is_some()).ok_or("Only plain table results can be edited.")?;
    let (_, capabilities) = table_edit::metadata(&stored.connection, &stored.password, base).await?;
    Ok(capabilities)
}
#[tauri::command]
async fn edit_rows(state: State<'_, Runtime>, cache_id: String, changes: Vec<table_edit::Change>, inserts: Option<Vec<table_edit::Insert>>, deletes: Option<Vec<usize>>, apply: bool) -> Result<Vec<table_edit::Statement>, String> {
    let inserts = inserts.unwrap_or_default(); let deletes = deletes.unwrap_or_default();
    if changes.len() + inserts.len() + deletes.len() > 100 { return Err("At most 100 changes per submission.".into()); }
    let stored = stored_result(&state, &cache_id)?;
    if stored.join.is_some() && (!inserts.is_empty() || !deletes.is_empty()) { return Err("JOIN results support updates only. Open the source table to insert or delete rows.".into()); }
    let mut rows = vec![]; let mut columns = vec![];
    for row_index in changes.iter().map(|c| c.row_index).chain(deletes.iter().copied()) {
        if rows.iter().any(|(index, _)| *index == row_index) { continue; }
        let page = stored.page(0, row_index, 1).await?;
        columns = page.result.sets.first().ok_or("Missing result set")?.columns.clone();
        rows.push((row_index, page.rows.first().ok_or("Row is unavailable")?.clone()));
    }
    let statements = if let Some(join) = &stored.join {
        join.plan(&stored.connection.engine,&columns,&rows,&changes)?
    } else {
        let base = stored.base_count_sql.as_deref().filter(|_| stored.browser.is_some()).ok_or("Only plain tables or supported JOIN results can be edited.")?;
        let (keys, capabilities) = table_edit::metadata(&stored.connection, &stored.password, base).await?;
        let table = base.split_once(" FROM ").ok_or("Unknown table")?.1;
        table_edit::plan_operations(table, &stored.connection.engine, &keys, &capabilities, &columns, &rows, &changes, &inserts, &deletes)?
    };
    if apply {
        // A cached snapshot can only be submitted once, including uncertain connection failures.
        if !state.edited_results.lock().map_err(|e| e.to_string())?.insert(cache_id) { return Err("This snapshot was already submitted. Reload results before editing again.".into()); }
        table_edit::apply(&stored.connection, &stored.password, &statements).await?;
    }
    Ok(statements)
}

#[tauri::command]
async fn result_column_keys(
    state: State<'_, Runtime>,
    cache_id: String,
) -> Result<Vec<column_keys::ColumnKey>, String> {
    let stored = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .get(&cache_id)
        .map(|(_, stored)| stored.clone())
        .ok_or("Results have expired.")?;
    if let Some(join)=&stored.join { return Ok(join.column_keys()); }
    let sql = stored
        .base_count_sql
        .as_ref()
        .ok_or("Key metadata is available for plain SELECT * FROM table queries.")?;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        column_keys::load(&stored.connection, &stored.password, sql),
    )
    .await
    .map_err(|_| "Key metadata lookup timed out.".to_string())?
}

#[tauri::command]
async fn result_count(
    state: State<'_, Runtime>,
    cache_id: String,
    set_index: usize,
) -> Result<u64, String> {
    let stored = {
        let mut results = state.results.lock().map_err(|e| e.to_string())?;
        let (accessed, stored) = results
            .get_mut(&cache_id)
            .ok_or("Results have expired. Run the query again.")?;
        *accessed = std::time::Instant::now();
        stored.clone()
    };
    let sql = stored.count_sql.as_ref().filter(|_| set_index == 0)
        .ok_or("Table count currently supports a single SELECT * FROM table statement. For filtered queries, joins or batches, run an explicit COUNT query.")?;
    let (tx, rx) = oneshot::channel();
    {
        let mut running = state.running.lock().map_err(|e| e.to_string())?;
        if running.contains_key(&cache_id) {
            return Err("A count is already running.".into());
        }
        running.insert(cache_id.clone(), tx);
    }
    let result = tokio::select! {
        result = async { match &stored.parameters { Some(params) => database::execute_params(&stored.connection, &stored.password, sql, params).await, None => database::execute(&stored.connection, &stored.password, sql).await } } => result,
        _ = rx => Err("Count cancelled.".into()),
        _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => Err("Count timed out after 120 seconds.".into()),
    };
    state
        .running
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&cache_id);
    let result = result?;
    let count = result
        .sets
        .first()
        .and_then(|set| set.rows.first())
        .and_then(|row| row.first())
        .and_then(|value| value.as_ref())
        .ok_or("The count query did not return a value.")?
        .parse::<u64>()
        .map_err(|_| "Invalid row count.")?;
    if count > 9_007_199_254_740_991 {
        return Err("Row count exceeds the supported display range.".into());
    }
    Ok(count)
}

fn stored_result(state: &Runtime, id: &str) -> Result<Arc<StoredResult>, String> {
    state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .get(id)
        .map(|(_, s)| s.clone())
        .ok_or("Results have expired. Run the query again.".into())
}
#[tauri::command]
async fn browse_table(
    state: State<'_, Runtime>,
    cache_id: String,
    request_id: String,
    filters: Vec<table_browser::Filter>,
    sort: Option<table_browser::Sort>,
    page_size: usize,
) -> Result<QueryResult, String> {
    if page_size == 0 || page_size > 10000 {
        return Err("Invalid page size".into());
    }
    let old = stored_result(&state, &cache_id)?;
    let base = old
        .browser
        .as_ref()
        .ok_or("Database filtering requires a plain table query.")?;
    let (tx, rx) = oneshot::channel();
    {
        let mut running = state.running.lock().map_err(|e| e.to_string())?;
        if running.contains_key(&request_id) {
            return Err("Duplicate request ID".into());
        }
        running.insert(request_id.clone(), tx);
    }
    let start = std::time::Instant::now();
    let fetch = async {
        old.cancel();
        let keys = column_keys::load(
            &old.connection,
            &old.password,
            old.base_count_sql
                .as_deref()
                .ok_or("Unknown source table")?,
        )
        .await?;
        let plan = table_browser::build(base, filters, sort, &keys, &old.connection.engine)?;
        let connection = old.connection.clone();
        let password = old.password.clone();
        let sql = plan.browser.sql.clone();
        let params = plan.browser.parameters.clone();
        let stored = Arc::new(StoredResult {
            join: None, join_error: None,
            connection: old.connection.clone(),
            password: old.password.clone(),
            count_sql: Some(plan.count),
            base_count_sql: old.base_count_sql.clone(),
            parameters: Some(params.clone()),
            browser: Some(plan.browser),
            session: result_session::start(move |events| {
                database::stream_params(connection, password, sql, params, events)
            })?,
        });
        let mut result = stored.first_page(page_size).await?.result;
        result.browser = stored.browser.clone();
        result.cache_id = Some(request_id.clone());
        result.elapsed_ms = start.elapsed().as_millis();
        let mut results = state.results.lock().map_err(|e| e.to_string())?;
        results.remove(&cache_id);
        if results.len() >= 4 {
            if let Some(id) = results
                .iter()
                .min_by_key(|(_, (time, _))| *time)
                .map(|(id, _)| id.clone())
            {
                if let Some((_, s)) = results.remove(&id) {
                    s.cancel();
                }
            }
        }
        results.insert(request_id.clone(), (std::time::Instant::now(), stored));
        Ok(result)
    };
    let result = tokio::select! {result=fetch=>result,_=rx=>Err("Query cancelled.".into()),_=tokio::time::sleep(std::time::Duration::from_secs(120))=>Err("Query timed out.".into())};
    state
        .running
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&request_id);
    result
}
#[derive(Serialize)]
struct Choice {
    value: Option<String>,
    count: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Choices {
    values: Vec<Choice>,
    has_more: bool,
}
#[tauri::command]
async fn table_values(
    state: State<'_, Runtime>,
    cache_id: String,
    column: String,
    search: String,
) -> Result<Choices, String> {
    let stored = stored_result(&state, &cache_id)?;
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let base = stored.browser.as_ref().ok_or("Unknown source table")?;
        let keys = column_keys::load(
            &stored.connection,
            &stored.password,
            stored
                .base_count_sql
                .as_deref()
                .ok_or("Unknown source table")?,
        )
        .await?;
        let (sql, params) =
            table_browser::choices(base, &column, &search, &keys, &stored.connection.engine)?;
        let result =
            database::execute_params(&stored.connection, &stored.password, &sql, &params).await?;
        let rows = &result.sets.first().ok_or("No values returned")?.rows;
        let values = rows
            .iter()
            .take(100)
            .map(|r| {
                Ok(Choice {
                    value: r.first().cloned().flatten(),
                    count: r
                        .get(1)
                        .and_then(|v| v.as_deref())
                        .ok_or("Missing value count")?
                        .parse::<u64>()
                        .map_err(|_| "Invalid value count")?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Choices {
            values,
            has_more: rows.len() > 100 || result.truncated,
        })
    })
    .await
    .map_err(|_| "Value lookup timed out.".to_string())?
}

#[tauri::command]
async fn result_references(
    state: State<'_, Runtime>,
    cache_id: String,
    column: String,
) -> Result<Vec<foreign_keys::Reference>, String> {
    let stored = stored_result(&state, &cache_id)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        foreign_keys::load(
            &stored.connection,
            &stored.password,
            stored
                .base_count_sql
                .as_deref()
                .ok_or("Navigation requires a plain table result")?,
            &column,
        ),
    )
    .await
    .map_err(|_| "Relationship lookup timed out".to_string())?
}
#[derive(Serialize)]
struct ReferenceResult {
    result: QueryResult,
    sql: String,
    name: String,
}
#[tauri::command]
async fn open_reference(
    state: State<'_, Runtime>,
    cache_id: String,
    column: String,
    reference_id: String,
    row_index: usize,
    request_id: String,
) -> Result<ReferenceResult, String> {
    let source = stored_result(&state, &cache_id)?;
    let (tx, rx) = oneshot::channel();
    {
        let mut running = state.running.lock().map_err(|e| e.to_string())?;
        if running.contains_key(&request_id) {
            return Err("Duplicate request ID".into());
        }
        running.insert(request_id.clone(), tx);
    }
    let started = std::time::Instant::now();
    let fetch = async {
        let references = foreign_keys::load(
            &source.connection,
            &source.password,
            source
                .base_count_sql
                .as_deref()
                .ok_or("Unknown source table")?,
            &column,
        )
        .await?;
        let reference = references
            .iter()
            .find(|r| r.id == reference_id)
            .ok_or("Foreign key no longer exists")?;
        let page = source.page(0, row_index, 1).await?;
        let row = page.rows.first().ok_or("Source row is unavailable")?;
        let columns = &page
            .result
            .sets
            .first()
            .ok_or("Source columns unavailable")?
            .columns;
        let filters = foreign_keys::filters(reference, columns, row)?;
        let engine = &source.connection.engine;
        let table = format!(
            "{}.{}",
            foreign_keys::quote(&reference.schema, engine),
            foreign_keys::quote(&reference.table, engine)
        );
        let table_sql = format!("SELECT * FROM {table}");
        let base_count =
            count_query::table_count(&table_sql, engine).ok_or("Unsupported referenced table")?;
        let keys = column_keys::load(&source.connection, &source.password, &base_count).await?;
        let base =
            table_browser::initial(&table_sql, engine).ok_or("Unsupported referenced table")?;
        let sql = format!(
            "{table_sql} WHERE {};",
            filters
                .iter()
                .map(|f| format!(
                    "{} = {}",
                    foreign_keys::quote(&f.column, engine),
                    foreign_keys::literal(&f.value, engine)
                ))
                .collect::<Vec<_>>()
                .join(" AND ")
        );
        let plan = table_browser::build(&base, filters, None, &keys, engine)?;
        let connection = source.connection.clone();
        let password = source.password.clone();
        let statement = plan.browser.sql.clone();
        let params = plan.browser.parameters.clone();
        let stored = Arc::new(StoredResult {
            join: None, join_error: None,
            connection: source.connection.clone(),
            password: source.password.clone(),
            base_count_sql: Some(base_count),
            count_sql: Some(plan.count),
            parameters: Some(params.clone()),
            browser: Some(plan.browser),
            session: result_session::start(move |events| {
                database::stream_params(connection, password, statement, params, events)
            })?,
        });
        let mut result = stored.first_page(500).await?.result;
        result.cache_id = Some(request_id.clone());
        result.browser = stored.browser.clone();
        result.elapsed_ms = started.elapsed().as_millis();
        let mut results = state.results.lock().map_err(|e| e.to_string())?;
        if results.len() >= 4 {
            if let Some(id) = results
                .iter()
                .filter(|(id, _)| *id != &cache_id)
                .min_by_key(|(_, (time, _))| *time)
                .map(|(id, _)| id.clone())
            {
                if let Some((_, s)) = results.remove(&id) {
                    s.cancel();
                }
            }
        }
        results.insert(request_id.clone(), (std::time::Instant::now(), stored));
        Ok(ReferenceResult {
            result,
            sql,
            name: format!("{}.{}", reference.schema, reference.table),
        })
    };
    let result = tokio::select! {result=fetch=>result,_=rx=>Err("Navigation cancelled".into()),_=tokio::time::sleep(std::time::Duration::from_secs(120))=>Err("Navigation timed out".into())};
    state
        .running
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&request_id);
    result
}

#[tauri::command]
fn release_results(state: State<'_, Runtime>, cache_id: String) -> Result<(), String> {
    if let Some(tx) = state
        .running
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&cache_id)
    {
        let _ = tx.send(());
    }
    if let Some((_, session)) = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&cache_id)
    {
        session.cancel();
    }
    Ok(())
}

#[tauri::command]
fn cancel_query(state: State<'_, Runtime>, request_id: String) -> Result<(), String> {
    if let Some((_, session)) = state
        .results
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&request_id)
    {
        session.cancel();
    }
    if let Some(tx) = state
        .running
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&request_id)
    {
        let _ = tx.send(());
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            if tauri::is_dev() {
                if let Some(window) = app.get_webview_window("main") {
                    window.set_title("DataGoblin (development)")?;
                }
            }
            Ok(())
        })
        .manage(Runtime::default())
        .manage(library::LibraryLock::default())
        .manage(history::HistoryLock::default())
        .invoke_handler(tauri::generate_handler![
            history::query_history,
            history::record_query_history,
            load_connections,
            save_connections,
            save_password,
            forget_password,
            query,
            cancel_query,
            result_page,
            result_count,
            result_column_keys,
            edit_capabilities,
            edit_rows,
            refresh_join_results,
            result_references,
            open_reference,
            browse_table,
            table_values,
            release_results,
            library::query_library,
            library::save_library_query,
            library::delete_library_query,
            library::set_query_folder
        ])
        .run(tauri::generate_context!())
        .expect("DataGoblin could not start");
}
