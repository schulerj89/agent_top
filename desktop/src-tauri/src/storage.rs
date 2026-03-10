#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_top_core::{Event, EventKind, RunSettings, SessionLifecycle};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredSession {
    pub id: String,
    pub thread_id: String,
    pub attempt_no: i64,
    pub title: String,
    pub prompt: String,
    pub workspace: String,
    pub codex_session_id: Option<String>,
    pub resume_ready: bool,
    pub lifecycle: SessionLifecycle,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_event_at: Option<i64>,
    pub last_message: Option<String>,
    pub total_events: usize,
    pub command_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
    pub settings: RunSettings,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: i64,
    pub session_id: String,
    pub ts: i64,
    pub kind: EventKind,
    pub message: String,
    pub payload_json: Option<String>,
    pub sequence_no: i64,
}

#[derive(Clone, Debug)]
pub struct CreateSessionInput {
    pub id: String,
    pub thread_id: String,
    pub prompt: String,
    pub workspace: String,
    pub lifecycle: SessionLifecycle,
    pub status: String,
    pub settings: RunSettings,
}

#[derive(Clone, Debug)]
pub struct SessionUpdate {
    pub lifecycle: SessionLifecycle,
    pub status: String,
    pub last_message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SessionRunUpdate {
    pub prompt: String,
    pub workspace: String,
    pub codex_session_id: Option<String>,
    pub lifecycle: SessionLifecycle,
    pub status: String,
    pub settings: RunSettings,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StoredThread {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub workspace: String,
    pub latest_run_id: String,
    pub active_run_id: Option<String>,
    pub attempt_count: usize,
    pub codex_session_id: Option<String>,
    pub lifecycle: SessionLifecycle,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_event_at: Option<i64>,
    pub last_message: Option<String>,
    pub total_events: usize,
    pub command_count: usize,
    pub warning_count: usize,
    pub error_count: usize,
    pub settings: RunSettings,
}

#[derive(Clone, Debug)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn init(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let connection = Connection::open(&self.path).map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                r#"
                pragma foreign_keys = on;

                create table if not exists threads (
                    id text primary key,
                    title text not null,
                    created_at integer not null
                );

                create table if not exists sessions (
                    id text primary key,
                    thread_id text not null,
                    attempt_no integer not null default 1,
                    title text not null,
                    prompt text not null,
                    workspace text not null,
                    codex_session_id text,
                    resume_ready integer not null default 0,
                    lifecycle text not null,
                    status text not null,
                    created_at integer not null,
                    updated_at integer not null,
                    last_event_at integer,
                    last_message text,
                    total_events integer not null default 0,
                    command_count integer not null default 0,
                    warning_count integer not null default 0,
                    error_count integer not null default 0,
                    model text not null,
                    sandbox text not null,
                    approval text not null,
                    bypass_approvals_and_sandbox integer not null default 0
                );

                create table if not exists events (
                    id integer primary key autoincrement,
                    session_id text not null references sessions(id) on delete cascade,
                    ts integer not null,
                    kind text not null,
                    message text not null,
                    payload_json text,
                    sequence_no integer not null
                );

                create index if not exists idx_sessions_updated_at
                    on sessions(updated_at desc);
                "#,
            )
            .map_err(|error| error.to_string())?;

        let session_columns = {
            let mut statement = connection
                .prepare("pragma table_info(sessions)")
                .map_err(|error| error.to_string())?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|error| error.to_string())?;

            columns
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
                .into_iter()
                .collect::<Vec<_>>()
        };

        if !session_columns.iter().any(|name| name == "codex_session_id") {
            connection
                .execute("alter table sessions add column codex_session_id text", [])
                .map_err(|error| error.to_string())?;
        }

        if !session_columns.iter().any(|name| name == "resume_ready") {
            connection
                .execute(
                    "alter table sessions add column resume_ready integer not null default 0",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }

        if !session_columns
            .iter()
            .any(|name| name == "bypass_approvals_and_sandbox")
        {
            connection
                .execute(
                    "alter table sessions add column bypass_approvals_and_sandbox integer not null default 0",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }

        if !session_columns.iter().any(|name| name == "thread_id") {
            connection
                .execute(
                    "alter table sessions add column thread_id text not null default ''",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }

        if !session_columns.iter().any(|name| name == "attempt_no") {
            connection
                .execute(
                    "alter table sessions add column attempt_no integer not null default 1",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }

        connection
            .execute(
                r#"
                insert or ignore into threads (id, title, created_at)
                select id, title, created_at from sessions
                "#,
                [],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "update sessions set thread_id = id where trim(thread_id) = ''",
                [],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "create index if not exists idx_sessions_thread_attempt on sessions(thread_id, attempt_no desc)",
                [],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "create index if not exists idx_events_session_sequence on events(session_id, sequence_no)",
                [],
            )
            .map_err(|error| error.to_string())?;
        connection
            .execute(
                "create index if not exists idx_events_session_ts on events(session_id, ts)",
                [],
            )
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    pub fn create_session(&self, input: &CreateSessionInput) -> Result<StoredSession, String> {
        let mut connection = self.open()?;
        let now = now_ms();
        let title = derive_title(&input.prompt);
        let tx = connection.transaction().map_err(|error| error.to_string())?;
        let attempt_no: i64 = tx
            .query_row(
                "select coalesce(max(attempt_no), 0) + 1 from sessions where thread_id = ?1",
                [input.thread_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;

        tx.execute(
            "insert or ignore into threads (id, title, created_at) values (?1, ?2, ?3)",
            params![input.thread_id, title, now],
        )
        .map_err(|error| error.to_string())?;

        tx.execute(
                r#"
                insert into sessions (
                    id, thread_id, attempt_no, title, prompt, workspace, lifecycle, status, created_at, updated_at,
                    codex_session_id, resume_ready, last_event_at, last_message, total_events, command_count, warning_count,
                    error_count, model, sandbox, approval, bypass_approvals_and_sandbox
                ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, null, 0, null, null, 0, 0, 0, 0, ?10, ?11, ?12, ?13)
                "#,
                params![
                    input.id,
                    input.thread_id,
                    attempt_no,
                    title,
                    input.prompt,
                    input.workspace,
                    lifecycle_to_str(input.lifecycle),
                    input.status,
                    now,
                    input.settings.model,
                    input.settings.sandbox,
                    input.settings.approval,
                    if input.settings.bypass_approvals_and_sandbox {
                        1
                    } else {
                        0
                    }
                ],
            )
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;

        self.get_session(&input.id)?
            .ok_or_else(|| "session insert succeeded but row was not found".to_string())
    }

    pub fn append_event(
        &self,
        session_id: &str,
        event: &Event,
        payload_json: Option<&str>,
    ) -> Result<StoredEvent, String> {
        let mut connection = self.open()?;
        let tx = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let sequence_no: i64 = tx
            .query_row(
                "select coalesce(max(sequence_no), 0) + 1 from events where session_id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let ts = now_ms();

        tx.execute(
            r#"
            insert into events (session_id, ts, kind, message, payload_json, sequence_no)
            values (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                session_id,
                ts,
                event_kind_to_str(event.kind),
                event.message,
                payload_json,
                sequence_no
            ],
        )
        .map_err(|error| error.to_string())?;

        tx.execute(
            r#"
            update sessions
            set updated_at = ?2,
                last_event_at = ?2,
                last_message = ?3,
                total_events = total_events + 1,
                command_count = command_count + ?4,
                warning_count = warning_count + ?5,
                error_count = error_count + ?6
            where id = ?1
            "#,
            params![
                session_id,
                ts,
                event.message,
                if event.kind == EventKind::Command {
                    1
                } else {
                    0
                },
                if event.kind == EventKind::Warning {
                    1
                } else {
                    0
                },
                if event.kind == EventKind::Error { 1 } else { 0 }
            ],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())?;

        self.list_events(session_id, None)?
            .into_iter()
            .last()
            .ok_or_else(|| "event insert succeeded but row was not found".to_string())
    }

    pub fn update_session(&self, session_id: &str, update: &SessionUpdate) -> Result<(), String> {
        self.open()?
            .execute(
                r#"
                update sessions
                set lifecycle = ?2,
                    status = ?3,
                    updated_at = ?4,
                    last_message = coalesce(?5, last_message)
                where id = ?1
                "#,
                params![
                    session_id,
                    lifecycle_to_str(update.lifecycle),
                    update.status,
                    now_ms(),
                    update.last_message
                ],
            )
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    pub fn prepare_session_run(
        &self,
        session_id: &str,
        update: &SessionRunUpdate,
    ) -> Result<bool, String> {
        let updated = self
            .open()?
            .execute(
                r#"
                update sessions
                set prompt = ?2,
                    workspace = ?3,
                    codex_session_id = ?4,
                    resume_ready = 0,
                    lifecycle = ?5,
                    status = ?6,
                    updated_at = ?7,
                    last_message = ?8,
                    model = ?9,
                    sandbox = ?10,
                    approval = ?11,
                    bypass_approvals_and_sandbox = ?12
                where id = ?1
                "#,
                params![
                    session_id,
                    update.prompt,
                    update.workspace,
                    update.codex_session_id,
                    lifecycle_to_str(update.lifecycle),
                    update.status,
                    now_ms(),
                    "waiting for first event",
                    update.settings.model,
                    update.settings.sandbox,
                    update.settings.approval,
                    if update.settings.bypass_approvals_and_sandbox {
                        1
                    } else {
                        0
                    }
                ],
            )
            .map_err(|error| error.to_string())?;

        Ok(updated > 0)
    }

    pub fn list_threads(&self, limit: Option<usize>) -> Result<Vec<StoredThread>, String> {
        let connection = self.open()?;
        let mut sql = thread_summary_query("");
        if limit.is_some() {
            sql.push_str(" limit ?1");
        }

        let mut statement = connection.prepare(&sql).map_err(|error| error.to_string())?;
        let rows = match limit {
            Some(value) => statement
                .query_map([value as i64], read_thread)
                .map_err(|error| error.to_string())?,
            None => statement
                .query_map([], read_thread)
                .map_err(|error| error.to_string())?,
        };

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn get_thread(&self, thread_id: &str) -> Result<Option<StoredThread>, String> {
        let connection = self.open()?;
        let sql = thread_summary_query("where t.id = ?1");
        connection
            .query_row(&sql, [thread_id], read_thread)
            .optional()
            .map_err(|error| error.to_string())
    }

    pub fn selected_run_id_for_thread(&self, thread_id: &str) -> Result<Option<String>, String> {
        let connection = self.open()?;
        connection
            .query_row(
                r#"
                select coalesce(
                    (
                        select id from sessions
                        where thread_id = ?1
                          and lifecycle in ('launching', 'running', 'cancelling')
                        order by updated_at desc, attempt_no desc
                        limit 1
                    ),
                    (
                        select id from sessions
                        where thread_id = ?1
                        order by attempt_no desc
                        limit 1
                    )
                )
                "#,
                [thread_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())
    }

    pub fn list_sessions(&self, limit: Option<usize>) -> Result<Vec<StoredSession>, String> {
        let connection = self.open()?;
        let sql = match limit {
            Some(_) => "select * from sessions order by updated_at desc, created_at desc limit ?1"
                .to_string(),
            None => "select * from sessions order by updated_at desc, created_at desc".to_string(),
        };

        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| error.to_string())?;
        let rows = match limit {
            Some(value) => statement
                .query_map([value as i64], read_session)
                .map_err(|error| error.to_string())?,
            None => statement
                .query_map([], read_session)
                .map_err(|error| error.to_string())?,
        };

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<StoredSession>, String> {
        self.open()?
            .query_row(
                "select * from sessions where id = ?1",
                [session_id],
                read_session,
            )
            .optional()
            .map_err(|error| error.to_string())
    }

    pub fn list_events(
        &self,
        session_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<StoredEvent>, String> {
        let connection = self.open()?;
        let sql = match limit {
            Some(_) => {
                "select * from events where session_id = ?1 order by sequence_no asc limit ?2"
                    .to_string()
            }
            None => {
                "select * from events where session_id = ?1 order by sequence_no asc".to_string()
            }
        };

        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| error.to_string())?;
        let rows = match limit {
            Some(value) => statement
                .query_map(params![session_id, value as i64], read_event)
                .map_err(|error| error.to_string())?,
            None => statement
                .query_map([session_id], read_event)
                .map_err(|error| error.to_string())?,
        };

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn delete_session(&self, session_id: &str) -> Result<bool, String> {
        let deleted = self
            .open()?
            .execute("delete from sessions where id = ?1", [session_id])
            .map_err(|error| error.to_string())?;

        Ok(deleted > 0)
    }

    pub fn delete_thread(&self, thread_id: &str) -> Result<bool, String> {
        let mut connection = self.open()?;
        let tx = connection.transaction().map_err(|error| error.to_string())?;
        tx.execute(
            "delete from events where session_id in (select id from sessions where thread_id = ?1)",
            [thread_id],
        )
        .map_err(|error| error.to_string())?;
        tx.execute("delete from sessions where thread_id = ?1", [thread_id])
            .map_err(|error| error.to_string())?;
        let deleted = tx
            .execute("delete from threads where id = ?1", [thread_id])
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(deleted > 0)
    }

    pub fn set_codex_session_id(&self, session_id: &str, codex_session_id: &str) -> Result<(), String> {
        self.open()?
            .execute(
                "update sessions set codex_session_id = ?2 where id = ?1",
                params![session_id, codex_session_id],
            )
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    pub fn set_resume_ready(&self, session_id: &str, resume_ready: bool) -> Result<(), String> {
        self.open()?
            .execute(
                "update sessions set resume_ready = ?2 where id = ?1",
                params![session_id, if resume_ready { 1 } else { 0 }],
            )
            .map_err(|error| error.to_string())?;

        Ok(())
    }

    fn open(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path).map_err(|error| error.to_string())?;
        connection
            .execute_batch("pragma foreign_keys = on;")
            .map_err(|error| error.to_string())?;
        Ok(connection)
    }
}

pub fn default_db_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|value| PathBuf::from(value).join(".config")))
        .unwrap_or_else(|| std::env::temp_dir().join("agent_top"));

    base.join("agent_top").join("sessions.sqlite3")
}

fn read_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredSession> {
    Ok(StoredSession {
        id: row.get("id")?,
        thread_id: row.get("thread_id")?,
        attempt_no: row.get("attempt_no")?,
        title: row.get("title")?,
        prompt: row.get("prompt")?,
        workspace: row.get("workspace")?,
        codex_session_id: row.get("codex_session_id")?,
        resume_ready: row.get::<_, i64>("resume_ready")? != 0,
        lifecycle: lifecycle_from_str(&row.get::<_, String>("lifecycle")?)?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_event_at: row.get("last_event_at")?,
        last_message: row.get("last_message")?,
        total_events: row.get("total_events")?,
        command_count: row.get("command_count")?,
        warning_count: row.get("warning_count")?,
        error_count: row.get("error_count")?,
        settings: RunSettings {
            model: row.get("model")?,
            sandbox: row.get("sandbox")?,
            approval: row.get("approval")?,
            bypass_approvals_and_sandbox: row
                .get::<_, i64>("bypass_approvals_and_sandbox")?
                != 0,
        },
    })
}

fn read_thread(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredThread> {
    Ok(StoredThread {
        id: row.get("thread_id")?,
        title: row.get("title")?,
        prompt: row.get("prompt")?,
        workspace: row.get("workspace")?,
        latest_run_id: row.get("latest_run_id")?,
        active_run_id: row.get("active_run_id")?,
        attempt_count: row.get("attempt_count")?,
        codex_session_id: row.get("codex_session_id")?,
        lifecycle: lifecycle_from_str(&row.get::<_, String>("lifecycle")?)?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_event_at: row.get("last_event_at")?,
        last_message: row.get("last_message")?,
        total_events: row.get("total_events")?,
        command_count: row.get("command_count")?,
        warning_count: row.get("warning_count")?,
        error_count: row.get("error_count")?,
        settings: RunSettings {
            model: row.get("model")?,
            sandbox: row.get("sandbox")?,
            approval: row.get("approval")?,
            bypass_approvals_and_sandbox: row
                .get::<_, i64>("bypass_approvals_and_sandbox")?
                != 0,
        },
    })
}

fn thread_summary_query(where_clause: &str) -> String {
    format!(
        r#"
        select
            t.id as thread_id,
            t.title as title,
            latest.id as latest_run_id,
            latest.prompt as prompt,
            latest.workspace as workspace,
            latest.codex_session_id as codex_session_id,
            latest.lifecycle as lifecycle,
            latest.status as status,
            t.created_at as created_at,
            latest.updated_at as updated_at,
            latest.last_event_at as last_event_at,
            latest.last_message as last_message,
            (
                select coalesce(sum(total_events), 0)
                from sessions counts
                where counts.thread_id = t.id
            ) as total_events,
            (
                select coalesce(sum(command_count), 0)
                from sessions counts
                where counts.thread_id = t.id
            ) as command_count,
            (
                select coalesce(sum(warning_count), 0)
                from sessions counts
                where counts.thread_id = t.id
            ) as warning_count,
            (
                select coalesce(sum(error_count), 0)
                from sessions counts
                where counts.thread_id = t.id
            ) as error_count,
            (
                select count(*)
                from sessions counts
                where counts.thread_id = t.id
            ) as attempt_count,
            (
                select active.id
                from sessions active
                where active.thread_id = t.id
                  and active.lifecycle in ('launching', 'running', 'cancelling')
                order by active.updated_at desc, active.attempt_no desc
                limit 1
            ) as active_run_id,
            latest.model as model,
            latest.sandbox as sandbox,
            latest.approval as approval,
            latest.bypass_approvals_and_sandbox as bypass_approvals_and_sandbox
        from threads t
        join sessions latest
            on latest.thread_id = t.id
           and latest.attempt_no = (
                select max(candidate.attempt_no)
                from sessions candidate
                where candidate.thread_id = t.id
           )
        {where_clause}
        order by latest.updated_at desc, t.created_at desc
        "#
    )
}

fn read_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    Ok(StoredEvent {
        id: row.get("id")?,
        session_id: row.get("session_id")?,
        ts: row.get("ts")?,
        kind: event_kind_from_str(&row.get::<_, String>("kind")?)?,
        message: row.get("message")?,
        payload_json: row.get("payload_json")?,
        sequence_no: row.get("sequence_no")?,
    })
}

fn derive_title(prompt: &str) -> String {
    let compact = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= 48 {
        compact
    } else {
        format!("{}...", &compact[..45])
    }
}

fn lifecycle_to_str(value: SessionLifecycle) -> &'static str {
    match value {
        SessionLifecycle::Launching => "launching",
        SessionLifecycle::Running => "running",
        SessionLifecycle::Cancelling => "cancelling",
        SessionLifecycle::Cancelled => "cancelled",
        SessionLifecycle::Completed => "completed",
        SessionLifecycle::Failed => "failed",
    }
}

fn lifecycle_from_str(value: &str) -> rusqlite::Result<SessionLifecycle> {
    match value {
        "launching" => Ok(SessionLifecycle::Launching),
        "running" => Ok(SessionLifecycle::Running),
        "cancelling" => Ok(SessionLifecycle::Cancelling),
        "cancelled" => Ok(SessionLifecycle::Cancelled),
        "completed" => Ok(SessionLifecycle::Completed),
        "failed" => Ok(SessionLifecycle::Failed),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn event_kind_to_str(value: EventKind) -> &'static str {
    match value {
        EventKind::Status => "status",
        EventKind::Command => "command",
        EventKind::File => "file",
        EventKind::Warning => "warning",
        EventKind::Error => "error",
        EventKind::Note => "note",
    }
}

fn event_kind_from_str(value: &str) -> rusqlite::Result<EventKind> {
    EventKind::parse(value).ok_or(rusqlite::Error::InvalidQuery)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn store() -> (tempfile::TempDir, SessionStore) {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path().join("db").join("sessions.sqlite3"));
        store.init().expect("init");
        (dir, store)
    }

    fn test_settings() -> RunSettings {
        RunSettings {
            model: "gpt-5".to_string(),
            sandbox: "workspace-write".to_string(),
            approval: "never".to_string(),
            bypass_approvals_and_sandbox: false,
        }
    }

    #[test]
    fn initializes_schema() {
        let (_dir, store) = store();
        let sessions = store.list_sessions(None).expect("list");
        assert!(sessions.is_empty());
        assert!(store.path().exists());
    }

    #[test]
    fn creates_and_lists_sessions_in_updated_order() {
        let (_dir, store) = store();
        let first = store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "first prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Launching,
                status: "Launching".to_string(),
                settings: test_settings(),
            })
            .expect("create first");
        let second = store
            .create_session(&CreateSessionInput {
                id: "run-2".to_string(),
                thread_id: "thread-2".to_string(),
                prompt: "second prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Running,
                status: "Running".to_string(),
                settings: test_settings(),
            })
            .expect("create second");

        store
            .update_session(
                &first.id,
                &SessionUpdate {
                    lifecycle: SessionLifecycle::Completed,
                    status: "Completed".to_string(),
                    last_message: Some("done".to_string()),
                },
            )
            .expect("update");

        let sessions = store.list_sessions(None).expect("list");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, first.id);
        assert_eq!(sessions[1].id, second.id);
    }

    #[test]
    fn appends_events_in_sequence_order_and_updates_counters() {
        let (_dir, store) = store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Running,
                status: "Running".to_string(),
                settings: test_settings(),
            })
            .expect("create session");

        store
            .append_event(
                "run-1",
                &Event::new("1", EventKind::Command, "cargo test"),
                Some(r#"{"k":"v"}"#),
            )
            .expect("append command");
        store
            .append_event(
                "run-1",
                &Event::new("2", EventKind::Warning, "stderr"),
                None,
            )
            .expect("append warning");

        let events = store.list_events("run-1", None).expect("events");
        let session = store
            .get_session("run-1")
            .expect("get session")
            .expect("session exists");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence_no, 1);
        assert_eq!(events[1].sequence_no, 2);
        assert_eq!(session.total_events, 2);
        assert_eq!(session.command_count, 1);
        assert_eq!(session.warning_count, 1);
        assert_eq!(session.error_count, 0);
        assert_eq!(session.last_message.as_deref(), Some("stderr"));
    }

    #[test]
    fn persists_session_updates() {
        let (_dir, store) = store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Launching,
                status: "Launching".to_string(),
                settings: test_settings(),
            })
            .expect("create session");

        store
            .update_session(
                "run-1",
                &SessionUpdate {
                    lifecycle: SessionLifecycle::Failed,
                    status: "Failed".to_string(),
                    last_message: Some("boom".to_string()),
                },
            )
            .expect("update session");

        let session = store
            .get_session("run-1")
            .expect("get session")
            .expect("session exists");
        assert_eq!(session.lifecycle, SessionLifecycle::Failed);
        assert_eq!(session.status, "Failed");
        assert_eq!(session.last_message.as_deref(), Some("boom"));
    }

    #[test]
    fn returns_error_for_invalid_database_path() {
        let dir = tempdir().expect("tempdir");
        let store = SessionStore::new(dir.path());
        let error = store.init().expect_err("directory path should fail");
        assert!(!error.is_empty());
    }

    #[test]
    fn deletes_sessions_and_cascades_events() {
        let (_dir, store) = store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Running,
                status: "Running".to_string(),
                settings: test_settings(),
            })
            .expect("create session");

        store
            .append_event("run-1", &Event::new("1", EventKind::Note, "hello"), None)
            .expect("append event");

        assert!(store.delete_session("run-1").expect("delete session"));
        assert!(store.get_session("run-1").expect("load session").is_none());
        assert!(store.list_events("run-1", None).expect("load events").is_empty());
        assert!(!store.delete_session("run-1").expect("delete missing session"));
    }

    #[test]
    fn prepares_existing_session_for_continued_run() {
        let (_dir, store) = store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "first prompt".to_string(),
                workspace: "c:/repo-a".to_string(),
                lifecycle: SessionLifecycle::Completed,
                status: "Completed".to_string(),
                settings: test_settings(),
            })
            .expect("create session");

        let updated = store
            .prepare_session_run(
                "run-1",
                &SessionRunUpdate {
                    prompt: "second prompt".to_string(),
                    workspace: "c:/repo-b".to_string(),
                    codex_session_id: None,
                    lifecycle: SessionLifecycle::Launching,
                    status: "Launching".to_string(),
                    settings: RunSettings {
                        model: "o4".to_string(),
                        sandbox: "danger-full-access".to_string(),
                        approval: "on-request".to_string(),
                        bypass_approvals_and_sandbox: true,
                    },
                },
            )
            .expect("prepare session");

        let session = store
            .get_session("run-1")
            .expect("get session")
            .expect("session exists");

        assert!(updated);
        assert_eq!(session.prompt, "second prompt");
        assert_eq!(session.workspace, "c:/repo-b");
        assert_eq!(session.codex_session_id, None);
        assert_eq!(session.lifecycle, SessionLifecycle::Launching);
        assert_eq!(session.status, "Launching");
        assert_eq!(session.settings.model, "o4");
        assert_eq!(session.settings.sandbox, "danger-full-access");
        assert_eq!(session.settings.approval, "on-request");
        assert!(session.settings.bypass_approvals_and_sandbox);
        assert_eq!(session.last_message.as_deref(), Some("waiting for first event"));
    }

    #[test]
    fn stores_codex_session_ids() {
        let (_dir, store) = store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Launching,
                status: "Launching".to_string(),
                settings: test_settings(),
            })
            .expect("create session");

        store
            .set_codex_session_id("run-1", "019ccdee-5bdb-7602-95df-d6edbfd0083c")
            .expect("set codex session id");

        let session = store
            .get_session("run-1")
            .expect("get session")
            .expect("session exists");

        assert_eq!(
            session.codex_session_id.as_deref(),
            Some("019ccdee-5bdb-7602-95df-d6edbfd0083c")
        );
        assert!(!session.resume_ready);
    }

    #[test]
    fn stores_resume_ready_state() {
        let (_dir, store) = store();
        store
            .create_session(&CreateSessionInput {
                id: "run-1".to_string(),
                thread_id: "thread-1".to_string(),
                prompt: "prompt".to_string(),
                workspace: "c:/repo".to_string(),
                lifecycle: SessionLifecycle::Completed,
                status: "Completed".to_string(),
                settings: test_settings(),
            })
            .expect("create session");

        store
            .set_resume_ready("run-1", true)
            .expect("set resume ready");

        let session = store
            .get_session("run-1")
            .expect("get session")
            .expect("session exists");

        assert!(session.resume_ready);
    }

    #[test]
    fn enables_foreign_keys_for_open_connections() {
        let (_dir, store) = store();
        let connection = store.open().expect("open store");
        let foreign_keys: i64 = connection
            .query_row("pragma foreign_keys", [], |row| row.get(0))
            .expect("pragma foreign_keys");

        assert_eq!(foreign_keys, 1);
    }
}
