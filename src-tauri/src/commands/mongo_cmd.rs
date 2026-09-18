use serde::Serialize;
use std::future::Future;
use std::process::Stdio;
use std::sync::Arc;
use tauri::State;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::commands::connection::{ensure_connection_writable, AppState};
use dbx_core::db::mongo_driver::MongoDocumentResult;

const MONGOSH_EVAL: &str = r#"(async () => {
  let source = require('fs').readFileSync(0, 'utf8');
  db = connect(process.env.DBX_MONGO_URI);
  const use = source.match(/^\s*use\s+([A-Za-z0-9_-]+)\s*(?:;|\r?\n|$)/i);
  if (use) {
    db = db.getSiblingDB(use[1]);
    source = source.slice(use[0].length);
  }
  const show = source.trim().match(/^show\s+(dbs|databases|collections|users)\s*;?$/i);
  if (show) {
    const kind = show[1].toLowerCase();
    source = kind === 'collections' ? 'db.getCollectionNames()' : kind === 'users' ? 'db.getUsers()' : 'db.adminCommand({listDatabases: 1}).databases';
  }
  let result = source.trim() ? await eval(source) : { database: db.getName() };
  if (result && typeof result.toArray === 'function') result = await result.toArray();
  print('__DBX_MONGOSH_RESULT__' + EJSON.stringify(result === undefined ? null : result, null, 0, { relaxed: true }));
})().catch(error => { print('__DBX_MONGOSH_ERROR__' + (error?.message || String(error))); process.exitCode = 1; })"#;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MongoshStatus {
    installed: bool,
    version: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MongoshResult {
    value: serde_json::Value,
    output: String,
}

#[tauri::command]
pub async fn mongo_shell_status() -> MongoshStatus {
    let mut command = Command::new("mongosh");
    command.arg("--version").kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    match tokio::time::timeout(std::time::Duration::from_secs(5), command.output()).await {
        Ok(Ok(output)) if output.status.success() => {
            MongoshStatus { installed: true, version: Some(String::from_utf8_lossy(&output.stdout).trim().to_string()) }
        }
        _ => MongoshStatus { installed: false, version: None },
    }
}

#[tauri::command]
pub async fn mongo_execute_mongosh(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    source: String,
    execution_id: Option<String>,
) -> Result<MongoshResult, String> {
    if dbx_core::query::connection_readonly_name(&state, &connection_id).await.is_some() {
        return Err("mongosh execution is unavailable for read-only connections; use DBX's built-in MongoDB commands"
            .to_string());
    }
    let config = state.configs.read().await.get(&connection_id).cloned().ok_or("Connection not found")?;
    if config.db_type != dbx_core::models::connection::DatabaseType::MongoDb {
        return Err("Not a MongoDB connection".to_string());
    }
    let mut config = dbx_core::connection::metadata_connection_config(&config);
    state.apply_session_credential(&config.clone(), &mut config, &connection_id);
    let (host, port) = state.connection_host_port(&connection_id, &config).await?;
    if !database.trim().is_empty() {
        config.database = Some(database);
    }
    let uri = config.connection_url_with_host(&host, port);
    let timeout_secs = config.query_timeout_secs.clamp(1, 300);
    run_cancellable(&state, execution_id, async move {
        let mut command = Command::new("mongosh");
        command
            .args(["--nodb", "--quiet", "--norc", "--eval", MONGOSH_EVAL])
            .env("DBX_MONGO_URI", &uri)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn().map_err(|error| format!("Unable to start mongosh from PATH: {error}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(source.as_bytes())
                .await
                .map_err(|error| format!("Unable to send script to mongosh: {error}"))?;
        }
        let output = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), child.wait_with_output())
            .await
            .map_err(|_| "mongosh query timed out".to_string())?
            .map_err(|error| format!("mongosh execution failed: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let shell_error = stdout.lines().find_map(|line| line.strip_prefix("__DBX_MONGOSH_ERROR__"));
        if !output.status.success() || shell_error.is_some() {
            let message = shell_error.unwrap_or(stderr.trim());
            return Err(message.replace(&uri, "[redacted MongoDB URI]"));
        }
        let marker = "__DBX_MONGOSH_RESULT__";
        let marker_at =
            stdout.rfind(marker).ok_or_else(|| format!("mongosh returned no structured result: {}", stderr.trim()))?;
        let value = serde_json::from_str(stdout[marker_at + marker.len()..].trim())
            .map_err(|error| format!("Unable to parse mongosh result: {error}"))?;
        Ok(MongoshResult { value, output: stdout[..marker_at].trim().to_string() })
    })
    .await
}

#[tauri::command]
pub fn mongo_parse_shell_command(source: String) -> Result<dbx_core::mongo_shell::MongoCommand, String> {
    dbx_core::mongo_shell::parse(&source)
}

async fn run_cancellable<T, F>(state: &Arc<AppState>, execution_id: Option<String>, future: F) -> Result<T, String>
where
    F: Future<Output = Result<T, String>>,
{
    let registered_query =
        execution_id.as_ref().filter(|id| !id.trim().is_empty()).map(|id| state.running_queries.register(id.clone()));
    if let Some(query) = registered_query.as_ref() {
        let token = query.token();
        tokio::select! {
            biased;
            _ = token.cancelled() => Err(dbx_core::query::canceled_error()),
            result = future => result,
        }
    } else {
        future.await
    }
}

#[tauri::command]
pub async fn mongo_list_databases(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    dbx_core::mongo_ops::mongo_list_databases_core(&state, &connection_id).await
}

#[tauri::command]
pub async fn mongo_list_collections(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
) -> Result<Vec<dbx_core::document_ops::CollectionInfo>, String> {
    dbx_core::mongo_ops::mongo_list_collections_core(&state, &connection_id, &database).await
}

#[tauri::command]
pub async fn mongo_create_database(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
) -> Result<(), String> {
    ensure_connection_writable(&state, &connection_id, "Create database").await?;
    dbx_core::mongo_ops::mongo_create_database_core(&state, &connection_id, &database).await
}

#[tauri::command]
pub async fn mongo_drop_database(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
) -> Result<(), String> {
    ensure_connection_writable(&state, &connection_id, "Drop database").await?;
    dbx_core::mongo_ops::mongo_drop_database_core(&state, &connection_id, &database).await
}

#[tauri::command]
pub async fn mongo_drop_collection(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
) -> Result<(), String> {
    ensure_connection_writable(&state, &connection_id, "Drop collection").await?;
    dbx_core::mongo_ops::mongo_drop_collection_core(&state, &connection_id, &database, &collection).await
}

#[tauri::command]
pub async fn mongo_rename_collection(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    new_name: String,
) -> Result<(), String> {
    ensure_connection_writable(&state, &connection_id, "Rename collection").await?;
    dbx_core::mongo_ops::mongo_rename_collection_core(&state, &connection_id, &database, &collection, &new_name).await
}

#[tauri::command]
pub async fn mongo_clone_collection(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    source_collection: String,
    target_collection: String,
) -> Result<dbx_core::db::mongo_driver::MongoCloneCollectionResult, String> {
    ensure_connection_writable(&state, &connection_id, "Clone collection").await?;
    dbx_core::mongo_ops::mongo_clone_collection_core(
        &state,
        &connection_id,
        &database,
        &source_collection,
        &target_collection,
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mongo_find_documents(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    skip: u64,
    limit: i64,
    filter: Option<String>,
    projection: Option<String>,
    sort: Option<String>,
    collation: Option<String>,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_read_allowed_by_id(state.inner(), &connection_id, &database).await?;
    }
    crate::commands::document_cmd::document_find_documents(
        state,
        connection_id,
        database,
        collection,
        skip,
        limit,
        filter,
        projection,
        sort,
        collation,
        None,
        None,
        execution_id,
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mongo_find_one(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    filter: Option<String>,
    projection: Option<String>,
    options: Option<String>,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    let app = state.inner().clone();
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_read_allowed_by_id(&app, &connection_id, &database).await?;
    }
    run_cancellable(
        &app,
        execution_id,
        dbx_core::mongo_ops::mongo_find_one_core(
            &app,
            &connection_id,
            &database,
            &collection,
            filter.as_deref(),
            projection.as_deref(),
            options.as_deref(),
        ),
    )
    .await
}

#[tauri::command]
pub async fn mongo_count_documents(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    filter: Option<String>,
    mode: Option<String>,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<u64, String> {
    let app = state.inner().clone();
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_read_allowed_by_id(&app, &connection_id, &database).await?;
    }
    crate::commands::document_cmd::run_cancellable(
        &app,
        execution_id,
        dbx_core::mongo_ops::mongo_count_documents_core(
            &app,
            &connection_id,
            &database,
            &collection,
            filter.as_deref(),
            mode.as_deref(),
        ),
    )
    .await
}

#[tauri::command]
pub async fn mongo_server_version(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<String, String> {
    let app = state.inner().clone();
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_read_allowed_by_id(&app, &connection_id, &database).await?;
    }
    run_cancellable(&app, execution_id, dbx_core::mongo_ops::mongo_server_version_core(&app, &connection_id, &database))
        .await
}

#[tauri::command]
pub async fn mongo_collection_stats(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    scale: Option<serde_json::Number>,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<dbx_core::db::mongo_driver::MongoCollectionStatsResult, String> {
    let app = state.inner().clone();
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_read_allowed_by_id(&app, &connection_id, &database).await?;
    }
    run_cancellable(
        &app,
        execution_id,
        dbx_core::mongo_ops::mongo_collection_stats_core(&app, &connection_id, &database, &collection, scale),
    )
    .await
}

#[tauri::command]
pub async fn mongo_aggregate_documents(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    pipeline_json: String,
    max_rows: Option<usize>,
    options_json: Option<String>,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    let app = state.inner().clone();
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_mongo_aggregate_allowed_by_id(
            &app,
            &connection_id,
            &database,
            &pipeline_json,
        )
        .await?;
    }
    run_cancellable(
        &app,
        execution_id,
        dbx_core::mongo_ops::mongo_aggregate_documents_core(
            &app,
            &connection_id,
            &database,
            &collection,
            &pipeline_json,
            max_rows,
            options_json.as_deref(),
        ),
    )
    .await
}

#[tauri::command]
pub async fn mongo_distinct(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    field: String,
    filter: Option<String>,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    let app = state.inner().clone();
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_read_allowed_by_id(&app, &connection_id, &database).await?;
    }
    run_cancellable(
        &app,
        execution_id,
        dbx_core::mongo_ops::mongo_distinct_core(
            &app,
            &connection_id,
            &database,
            &collection,
            &field,
            filter.as_deref(),
        ),
    )
    .await
}

/// Read-only listing of a collection's indexes with their full MongoDB options.
#[tauri::command]
pub async fn mongo_list_index_specs(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
) -> Result<Vec<dbx_core::db::mongo_driver::MongoIndexSpec>, String> {
    dbx_core::mongo_ops::mongo_list_index_specs_core(&state, &connection_id, &database, &collection).await
}

#[tauri::command]
pub async fn mongo_create_index(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    keys_json: String,
    options_json: Option<String>,
    mcp_request: Option<bool>,
) -> Result<serde_json::Value, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_dangerous_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Create index",
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Create index").await?;
    let name = dbx_core::mongo_ops::mongo_create_index_core(
        &state,
        &connection_id,
        &database,
        &collection,
        &keys_json,
        options_json.as_deref(),
    )
    .await?;
    Ok(serde_json::json!({ "name": name }))
}

#[tauri::command]
pub async fn mongo_create_user(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    user_json: String,
    write_concern_json: Option<String>,
    mcp_request: Option<bool>,
) -> Result<serde_json::Value, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_dangerous_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Create user",
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Create user").await?;
    let affected_rows = dbx_core::mongo_ops::mongo_create_user_core(
        &state,
        &connection_id,
        &database,
        &user_json,
        write_concern_json.as_deref(),
    )
    .await?;
    Ok(serde_json::json!({ "affected_rows": affected_rows }))
}

#[tauri::command]
pub async fn mongo_run_command(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    command_json: String,
    execution_id: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_dangerous_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Run MongoDB command",
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Run MongoDB command").await?;
    let app = state.inner().clone();
    run_cancellable(
        &app,
        execution_id,
        dbx_core::mongo_ops::mongo_run_command_core(&app, &connection_id, &database, &command_json),
    )
    .await
}

#[tauri::command]
pub async fn mongo_drop_indexes(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    indexes_json: Option<String>,
    single: bool,
    mcp_request: Option<bool>,
) -> Result<dbx_core::db::mongo_driver::MongoDropIndexesResult, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_dangerous_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Drop indexes",
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Drop indexes").await?;
    dbx_core::mongo_ops::mongo_drop_indexes_core(
        &state,
        &connection_id,
        &database,
        &collection,
        indexes_json.as_deref(),
        single,
    )
    .await
}

#[tauri::command]
pub async fn mongo_insert_document(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    doc_json: String,
    routing: Option<String>,
) -> Result<String, String> {
    crate::commands::document_cmd::document_insert_document(
        state,
        connection_id,
        database,
        collection,
        doc_json,
        routing,
        None,
    )
    .await
}

#[tauri::command]
pub async fn mongo_insert_documents(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    docs_json: String,
    mcp_request: Option<bool>,
) -> Result<u64, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_write_allowed_by_id(state.inner(), &connection_id, &database, "Insert")
            .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Insert").await?;
    dbx_core::mongo_ops::mongo_insert_documents_core(&state, &connection_id, &database, &collection, &docs_json).await
}

#[tauri::command]
pub async fn mongo_update_document(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    id: String,
    doc_json: String,
    routing: Option<String>,
) -> Result<u64, String> {
    crate::commands::document_cmd::document_update_document(
        state,
        connection_id,
        database,
        collection,
        id,
        doc_json,
        routing,
    )
    .await
}

#[tauri::command]
pub async fn mongo_update_documents(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    update_json: String,
    many: bool,
    options_json: Option<String>,
    mcp_request: Option<bool>,
) -> Result<u64, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_mongo_filtered_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Update",
            &filter_json,
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Update").await?;
    dbx_core::mongo_ops::mongo_update_documents_core(
        &state,
        &connection_id,
        &database,
        &collection,
        &filter_json,
        &update_json,
        many,
        options_json.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn mongo_delete_document(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    id: String,
    routing: Option<String>,
) -> Result<u64, String> {
    crate::commands::document_cmd::document_delete_document(
        state,
        connection_id,
        database,
        collection,
        id,
        routing,
        None,
    )
    .await
}

#[tauri::command]
pub async fn mongo_delete_documents(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    many: bool,
    mcp_request: Option<bool>,
) -> Result<u64, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_mongo_filtered_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Delete",
            &filter_json,
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Delete").await?;
    dbx_core::mongo_ops::mongo_delete_documents_core(&state, &connection_id, &database, &collection, &filter_json, many)
        .await
}

#[tauri::command]
pub async fn mongo_find_one_and_update(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    update_json: String,
    options_json: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_mongo_filtered_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Update",
            &filter_json,
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Update").await?;
    dbx_core::mongo_ops::mongo_find_one_and_update_core(
        &state,
        &connection_id,
        &database,
        &collection,
        &filter_json,
        &update_json,
        options_json.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn mongo_find_one_and_replace(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    replacement_json: String,
    options_json: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_mongo_filtered_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Update",
            &filter_json,
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Update").await?;
    dbx_core::mongo_ops::mongo_find_one_and_replace_core(
        &state,
        &connection_id,
        &database,
        &collection,
        &filter_json,
        &replacement_json,
        options_json.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn mongo_find_one_and_delete(
    state: State<'_, Arc<AppState>>,
    connection_id: String,
    database: String,
    collection: String,
    filter_json: String,
    options_json: Option<String>,
    mcp_request: Option<bool>,
) -> Result<MongoDocumentResult, String> {
    if mcp_request == Some(true) {
        crate::commands::mcp_bridge::ensure_mcp_mongo_filtered_write_allowed_by_id(
            state.inner(),
            &connection_id,
            &database,
            "Delete",
            &filter_json,
        )
        .await?;
    }
    ensure_connection_writable(&state, &connection_id, "Delete").await?;
    dbx_core::mongo_ops::mongo_find_one_and_delete_core(
        &state,
        &connection_id,
        &database,
        &collection,
        &filter_json,
        options_json.as_deref(),
    )
    .await
}
