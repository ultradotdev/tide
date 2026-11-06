//! Bounded provider-owned metadata operations. Provider stores are never written.
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, sync_channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::cli::{CliProvider, CliSession, epoch_millis_to_rfc3339, safe_session_id};

/// The longest line a `codex app-server` may send. A `thread/list` page is one
/// line holding up to 100 threads, each with a preview of its first message, which
/// Codex does not shorten: pages of half a megabyte are ordinary.
const MAX_FRAME: usize = 16 * 1024 * 1024;
/// The most a `codex app-server` may send in all, across every page.
const MAX_OUTPUT: usize = 64 * 1024 * 1024;

/// How long a `codex app-server` has to start and answer every request of one client.
/// Listing pages through every thread (about 2 s per 100 on a laptop, more on a busy
/// one), and a cold start can stall for seconds, so this is well above a quick reply.
const CODEX_BUDGET: Duration = Duration::from_secs(15);
/// A native process is killed this long after its client's budget runs out.
const WATCHDOG_GRACE: Duration = Duration::from_secs(3);
/// The lifetime of a temporary `opencode serve`.
const OPENCODE_LIFETIME: Duration = Duration::from_secs(8);

#[derive(Clone, Default)]
pub struct Cancellation(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
    fn cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }
}
thread_local! {static CANCELLATION: std::cell::RefCell<Option<Cancellation>>=const {std::cell::RefCell::new(None)};}
pub struct CancellationScope(Option<Cancellation>);
impl CancellationScope {
    pub fn enter(token: Cancellation) -> Self {
        Self(CANCELLATION.with(|current| current.replace(Some(token))))
    }
}
impl Drop for CancellationScope {
    fn drop(&mut self) {
        CANCELLATION.with(|current| current.replace(self.0.take()));
    }
}
pub(crate) fn current_cancellation() -> Option<Cancellation> {
    CANCELLATION.with(|current| current.borrow().clone())
}

struct OwnedProcess {
    child: Child,
    completed: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

impl std::ops::Deref for OwnedProcess {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}
impl std::ops::DerefMut for OwnedProcess {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let (done, wake) = &*self.completed;
        *done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        wake.notify_all();
        stop(&mut self.child);
    }
}

/// Spawns `command` in its own process group, killed after `lifetime` unless it
/// completes or is dropped first.
fn spawn(command: &mut Command, lifetime: Duration) -> Result<OwnedProcess, String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().map_err(|error| error.to_string())?;
    let completed = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let deadline = completed.clone();
    let pid = child.id();
    let cancellation = current_cancellation();
    std::thread::spawn(move || {
        let (done, wake) = &*deadline;
        let expires = Instant::now() + lifetime;
        let mut guard = done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*guard
            && Instant::now() < expires
            && !cancellation.as_ref().is_some_and(Cancellation::cancelled)
        {
            guard = wake
                .wait_timeout(guard, Duration::from_millis(25))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        if !*guard {
            #[cfg(unix)]
            {
                use nix::sys::signal::{Signal, kill};
                use nix::unistd::Pid;
                if let Some(group) = tide_core::process::process_group_target(pid) {
                    let _ = kill(Pid::from_raw(group), Signal::SIGKILL);
                }
            }
        }
    });
    Ok(OwnedProcess { child, completed })
}

fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        if let Some(group) = tide_core::process::process_group_target(child.id()) {
            let _ = kill(Pid::from_raw(group), Signal::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub struct CodexClient {
    _child: OwnedProcess,
    input: ChildStdin,
    output: Receiver<Result<Value, String>>,
    deadline: Instant,
    next_id: u64,
}

impl CodexClient {
    pub fn start(executable: &Path, workspace: &Path) -> Result<Self, String> {
        let deadline = Instant::now() + CODEX_BUDGET;
        let mut child = spawn(
            tide_core::process::scrub_tide_secrets(&mut Command::new(executable))
                .args(["app-server"])
                .current_dir(workspace)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null()),
            CODEX_BUDGET + WATCHDOG_GRACE,
        )?;
        let input = child.stdin.take().ok_or("missing app-server stdin")?;
        let stdout = child.stdout.take().ok_or("missing app-server stdout")?;
        let (sender, output) = sync_channel(16);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut total = 0;
            loop {
                let mut bytes = Vec::new();
                let result = (&mut reader)
                    .take((MAX_FRAME + 1) as u64)
                    .read_until(b'\n', &mut bytes);
                let count = match result {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(error) => {
                        let _ = sender.send(Err(error.to_string()));
                        break;
                    }
                };
                total += count;
                if count > MAX_FRAME || total > MAX_OUTPUT {
                    let _ =
                        sender.send(Err("Codex app-server metadata output limit exceeded".into()));
                    break;
                }
                if let Ok(value) = serde_json::from_slice::<Value>(&bytes)
                    && value.get("id").is_some()
                    && sender.send(Ok(value)).is_err()
                {
                    break;
                }
            }
        });
        let mut client = Self {
            _child: child,
            input,
            output,
            deadline,
            next_id: 0,
        };
        client.request("initialize",json!({"clientInfo":{"name":"tide","title":"Tide","version":env!("CARGO_PKG_VERSION")}}))?;
        writeln!(client.input, "{}", json!({"method":"initialized"}))
            .map_err(|error| error.to_string())?;
        client.input.flush().map_err(|error| error.to_string())?;
        Ok(client)
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        writeln!(
            self.input,
            "{}",
            json!({"method":method,"id":id,"params":params})
        )
        .map_err(|error| error.to_string())?;
        self.input.flush().map_err(|error| error.to_string())?;
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Codex app-server timed out".into());
            }
            let response = self
                .output
                .recv_timeout(remaining)
                .map_err(|_| "Codex app-server closed or timed out".to_owned())??;
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                return Err(error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex operation failed")
                    .chars()
                    .take(300)
                    .collect());
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| "Codex response has no result".into());
        }
    }

    pub fn sessions(&mut self, _workspace: &Path) -> Result<Vec<CliSession>, String> {
        let mut sessions = Vec::new();
        let mut cursor = Value::Null;
        let mut visited = std::collections::HashSet::new();
        for _ in 0..20 {
            let result = self.request(
                "thread/list",
                json!({"cursor":cursor,"limit":100,"sortKey":"updated_at","sortDirection":"desc"}),
            )?;
            let rows = result
                .get("data")
                .and_then(Value::as_array)
                .ok_or("Codex returned an unsupported thread list")?;
            for row in rows.iter().take(100) {
                let Some(id) = row
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| safe_session_id(id))
                else {
                    continue;
                };
                let text = |key: &str| {
                    row.get(key)
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                };
                sessions.push(CliSession {
                    provider_id: CliProvider::Codex.id().into(),
                    session_id: id.into(),
                    title: text("name")
                        .or_else(|| text("preview"))
                        .unwrap_or_else(|| format!("Codex session {}", &id[..id.len().min(8)]))
                        .chars()
                        .take(256)
                        .collect(),
                    cwd: text("cwd"),
                    model: text("model").or_else(|| text("modelProvider")),
                    updated_at: row
                        .get("updatedAt")
                        .and_then(Value::as_i64)
                        .and_then(|time| {
                            epoch_millis_to_rfc3339(if time < 10_000_000_000 {
                                time.saturating_mul(1000)
                            } else {
                                time
                            })
                        }),
                });
            }
            cursor = result.get("nextCursor").cloned().unwrap_or(Value::Null);
            if cursor.is_null() || cursor.as_str() == Some("") {
                return Ok(sessions);
            }
            if !visited.insert(cursor.to_string()) {
                return Err("Codex repeated a pagination cursor".into());
            }
        }
        Err("Codex session catalog exceeds 20 pages".into())
    }
}

/// Temporary authenticated loopback server used only for supported native actions.
pub struct OpenCodeClient {
    child: OwnedProcess,
    origin: String,
    password: String,
    client: reqwest::Client,
}

enum StartFailure {
    /// The server exited early, typically because another process took the
    /// probed port between our probe and its bind. Worth retrying.
    Exited,
    Fatal(String),
}

impl From<String> for StartFailure {
    fn from(error: String) -> Self {
        Self::Fatal(error)
    }
}

impl OpenCodeClient {
    pub async fn start(executable: &Path, workspace: &Path) -> Result<Self, String> {
        for _ in 0..3 {
            match Self::start_once(executable, workspace).await {
                Ok(server) => return Ok(server),
                Err(StartFailure::Exited) => {}
                Err(StartFailure::Fatal(error)) => return Err(error),
            }
        }
        Err("OpenCode server exited before becoming ready".into())
    }

    async fn start_once(executable: &Path, workspace: &Path) -> Result<Self, StartFailure> {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        drop(listener);
        let password = crate::cli::new_uuid();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        // A liveness pipe closes even after abrupt parent exit. The shell owns
        // the process group and kills it when that pipe closes or when the
        // server exits, so an early exit (e.g. a lost port race) is observable.
        // Executable/arguments remain separate positional parameters.
        let child = spawn(
            tide_core::process::scrub_tide_secrets(&mut Command::new("/bin/sh"))
                .args([
                    "-c",
                    "exec 3<&0; { /bin/cat <&3 >/dev/null; kill -KILL -$$; } & \"$@\" </dev/null 3<&-; kill -KILL -$$",
                    "tide-provider",
                ])
                .arg(executable)
                .args([
                    "serve",
                    "--hostname",
                    "127.0.0.1",
                    "--port",
                    &port.to_string(),
                ])
                .current_dir(workspace)
                .env("OPENCODE_SERVER_USERNAME", "tide")
                .env("OPENCODE_SERVER_PASSWORD", &password)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
            OPENCODE_LIFETIME,
        )?;
        let mut server = Self {
            child,
            origin: format!("http://127.0.0.1:{port}"),
            password,
            client,
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        for _ in 0..40 {
            if Instant::now() >= deadline {
                break;
            }
            if server
                .child
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                return Err(StartFailure::Exited);
            }
            if server
                .request(reqwest::Method::GET, "/global/health", None)
                .await
                .is_ok_and(|v| v.get("healthy").and_then(Value::as_bool) == Some(true))
            {
                return Ok(server);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(StartFailure::Fatal(
            "OpenCode server did not become ready".into(),
        ))
    }

    pub async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.origin))
            .basic_auth("tide", Some(&self.password))
            .timeout(if path == "/global/health" {
                Duration::from_millis(200)
            } else {
                Duration::from_secs(2)
            });
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| "OpenCode request failed".to_owned())?;
        if !response.status().is_success() {
            return Err(format!("OpenCode operation returned {}", response.status()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "OpenCode response failed".to_owned())?
        {
            if bytes.len() + chunk.len() > MAX_FRAME {
                return Err("OpenCode metadata limit exceeded".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| "OpenCode returned malformed metadata".into())
    }
}

pub fn rename_session(
    provider: CliProvider,
    executable: &Path,
    workspace: &Path,
    id: &str,
    title: &str,
) -> Result<(), String> {
    if !safe_session_id(id) || title.trim().is_empty() || title.len() > 512 {
        return Err("Invalid session name or identity".into());
    }
    match provider {
        CliProvider::Codex => {
            CodexClient::start(executable, workspace)?
                .request("thread/name/set", json!({"threadId":id,"name":title}))?;
            Ok(())
        }
        CliProvider::OpenCode => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?
            .block_on(async {
                OpenCodeClient::start(executable, workspace)
                    .await?
                    .request(
                        reqwest::Method::PATCH,
                        &format!("/session/{id}"),
                        Some(json!({"title":title})),
                    )
                    .await?;
                Ok(())
            }),
        _ => Err("Provider does not support native rename".into()),
    }
}

pub fn create_session(
    provider: CliProvider,
    executable: &Path,
    workspace: &Path,
) -> Result<(Vec<String>, Option<String>), String> {
    if provider == CliProvider::Codex {
        // The native app-server creates a persistent thread before the CLI is
        // launched, eliminating ambiguous "new ID since last refresh" matching.
        // Protocol: https://learn.chatgpt.com/docs/app-server#start-a-thread
        let result = CodexClient::start(executable, workspace)?.request(
            "thread/start",
            json!({"cwd":workspace.display().to_string()}),
        )?;
        let id = result
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .filter(|id| safe_session_id(id))
            .ok_or("Codex did not return a valid thread identity")?
            .to_owned();
        return Ok((provider.launch_args(workspace, Some(&id)), Some(id)));
    }
    if provider != CliProvider::OpenCode {
        return Ok(provider.prepare_new(workspace));
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(async {
            let server = OpenCodeClient::start(executable, workspace).await?;
            let session = server
                .request(reqwest::Method::POST, "/session", Some(json!({})))
                .await?;
            let id = session
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| safe_session_id(id))
                .ok_or("OpenCode did not return a valid session identity")?
                .to_owned();
            Ok((vec!["--session".into(), id.clone()], Some(id)))
        })
}

pub fn archive_codex(executable: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    if !safe_session_id(id) {
        return Err("Invalid session identity".into());
    }
    CodexClient::start(executable, workspace)?.request("thread/archive", json!({"threadId":id}))?;
    Ok(())
}

pub fn delete_opencode(executable: &Path, workspace: &Path, id: &str) -> Result<(), String> {
    if !safe_session_id(id) {
        return Err("Invalid session identity".into());
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(async {
            OpenCodeClient::start(executable, workspace)
                .await?
                .request(reqwest::Method::DELETE, &format!("/session/{id}"), None)
                .await?;
            Ok(())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_native_catalog_pages_preserve_real_workspaces_and_native_actions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("codex");
        let log = root.path().join("calls.jsonl");
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let script=r#"#!/usr/bin/python3
import json,sys
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r:continue
 with open(__LOG__,'a') as f:f.write(json.dumps(r)+'\n')
 if r['method']=='thread/list':
  page=r['params']['cursor']
  result={'data':[{'id':'native-2' if page else 'native-1','name':'Actual title','cwd':__WORKSPACE__,'model':'native-model','updatedAt':1700000000}],'nextCursor':None if page else 'page-two'}
 elif r['method']=='thread/start':result={'thread':{'id':'native-new'}}
 else:result={}
 print(json.dumps({'id':r['id'],'result':result}),flush=True)
"#.replace("__LOG__",&serde_json::to_string(&log.display().to_string()).unwrap()).replace("__WORKSPACE__",&serde_json::to_string(&workspace.display().to_string()).unwrap());
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut client = CodexClient::start(&executable, &workspace).unwrap();
        let sessions = client.sessions(&workspace).unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|session| session.cwd.as_deref()
            == Some(workspace.to_str().unwrap())
            && session.model.as_deref() == Some("native-model")));
        client
            .request(
                "thread/name/set",
                json!({"threadId":"native-1","name":"Renamed"}),
            )
            .unwrap();
        client
            .request("thread/archive", json!({"threadId":"native-1"}))
            .unwrap();
        drop(client);
        let (args, id) = create_session(CliProvider::Codex, &executable, &workspace).unwrap();
        assert_eq!(id.as_deref(), Some("native-new"));
        assert_eq!(
            args,
            vec![
                "resume",
                "-C",
                workspace.to_str().unwrap(),
                "--",
                "native-new"
            ]
        );
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(
            calls.contains("page-two")
                && calls.contains("Renamed")
                && calls.contains("thread/archive")
        );
    }

    #[test]
    fn codex_protocol_output_and_repeated_cursors_are_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("codex");
        let script = r#"#!/usr/bin/python3
import json,sys
for line in sys.stdin:
 r=json.loads(line)
 if 'id' in r:print(json.dumps({'id':r['id'],'result':{'data':[],'nextCursor':'repeat'} if r['method']=='thread/list' else {}}),flush=True)
"#;
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let start = Instant::now();
        let mut client = CodexClient::start(&executable, root.path()).unwrap();
        assert!(
            client
                .sessions(root.path())
                .unwrap_err()
                .contains("repeated")
        );
        drop(client);
        std::fs::write(
            &executable,
            "#!/usr/bin/python3\nimport sys\nsys.stdout.write('x'*(16*1024*1024+1));sys.stdout.flush()\n",
        )
        .unwrap();
        assert!(
            CodexClient::start(&executable, root.path())
                .err()
                .unwrap()
                .contains("limit")
        );
        assert!(start.elapsed() < Duration::from_secs(6));
    }

    #[test]
    fn codex_pages_with_long_previews_are_listed() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("codex");
        // One page of 100 threads, each previewing 5 KB: about half a megabyte on
        // one line, as a real catalog sends.
        let script = r#"#!/usr/bin/python3
import json,sys
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r:continue
 rows=[{'id':'thread-%d'%i,'preview':'p'*5000} for i in range(100)]
 print(json.dumps({'id':r['id'],'result':{'data':rows,'nextCursor':None} if r['method']=='thread/list' else {}}),flush=True)
"#;
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut client = CodexClient::start(&executable, root.path()).unwrap();
        let sessions = client.sessions(root.path()).unwrap();
        assert_eq!(sessions.len(), 100);
        assert_eq!(sessions[0].title.chars().count(), 256);
    }

    #[test]
    fn opencode_native_create_rename_delete_use_authenticated_loopback_and_cleanup() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("opencode");
        let log = root.path().join("calls");
        let script=r#"#!/usr/bin/python3
import os,sys,json,base64
from http.server import HTTPServer,BaseHTTPRequestHandler
expected='Basic '+base64.b64encode(('tide:'+os.environ['OPENCODE_SERVER_PASSWORD']).encode()).decode()
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def handle_request(self):
  if self.headers.get('Authorization')!=expected:self.send_response(401);self.end_headers();return
  body=self.rfile.read(int(self.headers.get('Content-Length','0')))
  with open(__LOG__,'a') as f:f.write(self.command+' '+self.path+' '+body.decode()+'\n')
  value={'healthy':True} if self.path=='/global/health' else {'id':'native-open','title':'Renamed'}
  data=json.dumps(value).encode();self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
 do_GET=handle_request
 do_POST=handle_request
 do_PATCH=handle_request
 do_DELETE=handle_request
port=int(sys.argv[sys.argv.index('--port')+1]);HTTPServer(('127.0.0.1',port),Handler).serve_forever()
"#.replace("__LOG__",&serde_json::to_string(&log.display().to_string()).unwrap());
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (args, id) = create_session(CliProvider::OpenCode, &executable, root.path()).unwrap();
        assert_eq!(id.as_deref(), Some("native-open"));
        assert_eq!(args, vec!["--session", "native-open"]);
        rename_session(
            CliProvider::OpenCode,
            &executable,
            root.path(),
            "native-open",
            "Renamed",
        )
        .unwrap();
        delete_opencode(&executable, root.path(), "native-open").unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut server = OpenCodeClient::start(&executable, root.path())
                .await
                .unwrap();
            let address = server.origin.strip_prefix("http://").unwrap().to_owned();
            // Closing the owner pipe exercises the abrupt-owner-exit path,
            // independently of OwnedProcess::drop's explicit kill.
            drop(server.child.stdin.take());
            let deadline = Instant::now() + Duration::from_secs(1);
            while server.child.try_wait().unwrap().is_none() {
                assert!(Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(std::net::TcpStream::connect(address).is_err());
        });
        let calls = std::fs::read_to_string(log).unwrap();
        assert!(
            calls.contains("POST /session")
                && calls.contains("PATCH /session/native-open")
                && calls.contains("DELETE /session/native-open")
        );
        assert!(!calls.contains("Authorization"));
    }

    #[test]
    fn opencode_start_retries_when_the_server_fails_to_bind_its_port() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("opencode");
        let marker = root.path().join("first-attempt");
        let script = r#"#!/usr/bin/python3
import os,sys,json
from http.server import HTTPServer,BaseHTTPRequestHandler
marker=__MARKER__
if not os.path.exists(marker):
 open(marker,'w').close()
 sys.exit('EADDRINUSE')
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_GET(self):
  data=json.dumps({'healthy':True}).encode();self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
 def do_POST(self):
  data=json.dumps({'id':'after-retry'}).encode();self.send_response(200);self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
port=int(sys.argv[sys.argv.index('--port')+1]);HTTPServer(('127.0.0.1',port),Handler).serve_forever()
"#
        .replace(
            "__MARKER__",
            &serde_json::to_string(&marker.display().to_string()).unwrap(),
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (args, id) = create_session(CliProvider::OpenCode, &executable, root.path()).unwrap();
        assert!(marker.exists());
        assert_eq!(id.as_deref(), Some("after-retry"));
        assert_eq!(args, vec!["--session", "after-retry"]);
    }

    #[test]
    fn dropping_a_process_with_a_poisoned_completion_lock_does_not_panic() {
        let process = spawn(Command::new("/bin/sleep").arg("30"), OPENCODE_LIFETIME).unwrap();
        let completed = process.completed.clone();
        let _ = std::thread::spawn(move || {
            let _guard = completed.0.lock().unwrap();
            panic!("poison the completion lock");
        })
        .join();
        assert!(process.completed.0.is_poisoned());
        let pid = tide_core::process::raw_pid(process.id()).unwrap();
        drop(process);
        assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err());
    }

    #[test]
    fn app_cancellation_stops_a_hung_native_process_without_waiting_for_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("codex");
        let pidfile = root.path().join("pid");
        let script = format!(
            "#!/usr/bin/python3\nimport os,time\nwith open({0}+'.tmp','w') as f:f.write(str(os.getpid()))\nos.replace({0}+'.tmp',{0})\ntime.sleep(60)\n",
            serde_json::to_string(&pidfile.display().to_string()).unwrap()
        );
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let cancellation = Cancellation::default();
        let token = cancellation.clone();
        let workspace = root.path().to_path_buf();
        let thread = std::thread::spawn(move || {
            let _scope = CancellationScope::enter(token);
            assert!(CodexClient::start(&executable, &workspace).is_err());
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pidfile.is_file() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let pid = std::fs::read_to_string(pidfile)
            .unwrap()
            .parse::<i32>()
            .unwrap();
        let start = Instant::now();
        cancellation.cancel();
        thread.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err());
    }
}
