//! Polkit authentication agent. Rune registers on the system bus as the agent for
//! its login session; polkitd asks it to authenticate, and the setuid
//! `polkit-agent-helper-1` runs PAM with the password typed into Rune's dialog.

use std::{collections::HashMap, fmt, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command as Process},
    sync::{mpsc, oneshot, watch},
};
use tracing::{info, warn};
use zbus::{
    Connection,
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const PATH: &str = "/org/rune/PolkitAgent";
const POLKIT: &str = "org.freedesktop.PolicyKit1";
const HELPERS: [&str; 3] = [
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
    "/usr/lib/policykit-1/polkit-agent-helper-1",
];
const MAX_TEXT: usize = 512;
const MAX_USERS: usize = 32;
const MAX_QUEUED: usize = 8;
const MAX_LINE: u64 = 4096;
const MAX_PASSWD: u64 = 4 * 1024 * 1024;
const HELPER_EXIT: Duration = Duration::from_secs(5);

/// A typed response for PAM. Its `Debug` never prints the text.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: String) -> Self {
        Self(text)
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// The authentication the dialog shows; `None` in the channel while there's none.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Request {
    pub cookie: String,
    pub action: String,
    pub message: String,
    /// Users who may authenticate, at least one.
    pub users: Vec<String>,
    /// The suggested user: the one logged in, when listed.
    pub user: usize,
    /// PAM's prompt, such as "Password: ".
    pub prompt: String,
    /// Whether the answer may be shown as typed (a user name, not a password).
    pub echo: bool,
    pub info: Option<String>,
    pub error: Option<String>,
    /// PAM is checking the last response.
    pub busy: bool,
    /// Failed attempts so far; the dialog clears its field when this changes.
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Answer the current prompt as `users[user]`.
    Respond {
        cookie: String,
        user: usize,
        response: Secret,
    },
    Cancel {
        cookie: String,
    },
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    Cancelled(String),
    NotAuthorized(String),
}

struct Job {
    request: Request,
    done: oneshot::Sender<Result<(), AgentError>>,
}

struct Agent {
    jobs: mpsc::Sender<Job>,
    commands: mpsc::UnboundedSender<Command>,
}

impl Agent {
    /// Only polkitd may drive the dialog; anyone else could put up a fake one.
    async fn check_sender(header: &Header<'_>, conn: &Connection) -> Result<(), AgentError> {
        let owner = zbus::fdo::DBusProxy::new(conn)
            .await?
            .get_name_owner(zbus::names::WellKnownName::from_static_str_unchecked(POLKIT).into())
            .await
            .map_err(|e| AgentError::NotAuthorized(format!("polkitd is not on the bus: {e}")))?;
        match header.sender() {
            Some(sender) if *sender == *owner => Ok(()),
            _ => Err(AgentError::NotAuthorized("caller is not polkitd".into())),
        }
    }
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    #[expect(
        clippy::too_many_arguments,
        reason = "signature fixed by the polkit agent interface"
    )]
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        _icon_name: String,
        _details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> Result<(), AgentError> {
        Self::check_sender(&header, conn).await?;
        let uids = user_ids(&identities);
        let passwd = read_passwd().await;
        let current = std::env::var("USER").unwrap_or_default();
        let request = request(&action_id, &message, &cookie, &uids, &passwd, &current)
            .ok_or_else(|| AgentError::Failed("no listed user can authenticate".into()))?;
        let (done, outcome) = oneshot::channel();
        self.jobs
            .try_send(Job { request, done })
            .map_err(|_| AgentError::Failed("too many authentications waiting".into()))?;
        outcome
            .await
            .unwrap_or_else(|_| Err(AgentError::Failed("agent stopped".into())))
    }

    async fn cancel_authentication(
        &self,
        cookie: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> Result<(), AgentError> {
        Self::check_sender(&header, conn).await?;
        self.commands
            .send(Command::Cancel {
                cookie: clean(&cookie),
            })
            .map_err(|_| AgentError::Failed("agent stopped".into()))
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.PolicyKit1.Authority",
    default_service = "org.freedesktop.PolicyKit1",
    default_path = "/org/freedesktop/PolicyKit1/Authority"
)]
trait Authority {
    fn register_authentication_agent(
        &self,
        subject: &(&str, HashMap<&str, Value<'_>>),
        locale: &str,
        object_path: &str,
    ) -> zbus::Result<()>;
}

/// Exports the agent on the system bus and registers it for this login session.
/// The channel stays `None` (and no dialog ever shows) if registration fails,
/// for example because another agent already serves the session.
pub async fn spawn(
    system: Connection,
) -> (
    watch::Receiver<Option<Request>>,
    mpsc::UnboundedSender<Command>,
) {
    let (state, rx) = watch::channel(None);
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (jobs, job_rx) = mpsc::channel(MAX_QUEUED);
    let agent = Agent {
        jobs,
        commands: commands.clone(),
    };
    if let Err(e) = system.object_server().at(PATH, agent).await {
        warn!("polkit: cannot export {PATH}: {e}");
        return (rx, commands);
    }
    if let Err(e) = register(&system).await {
        warn!("polkit: agent not registered ({e}); another agent may be running");
        return (rx, commands);
    }
    info!("polkit agent running");
    tokio::spawn(serve(job_rx, command_rx, state));
    (rx, commands)
}

async fn register(system: &Connection) -> zbus::Result<()> {
    let session = session_id(system).await?;
    let locale = std::env::var("LANG").unwrap_or_default();
    let subject = (
        "unix-session",
        HashMap::from([("session-id", Value::from(session.as_str()))]),
    );
    AuthorityProxy::new(system)
        .await?
        .register_authentication_agent(&subject, &locale, PATH)
        .await
}

/// `XDG_SESSION_ID`, or logind's display session for this user when Rune was
/// started outside the session (as a systemd user service, say).
async fn session_id(system: &Connection) -> zbus::Result<String> {
    if let Ok(id) = std::env::var("XDG_SESSION_ID")
        && !id.is_empty()
    {
        return Ok(id);
    }
    let user = zbus::Proxy::new(
        system,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/user/self",
        "org.freedesktop.login1.User",
    )
    .await?;
    let (id, _): (String, OwnedObjectPath) = user.get_property("Display").await?;
    if id.is_empty() {
        return Err(zbus::Error::Failure("no graphical login session".into()));
    }
    Ok(id)
}

/// The `unix-user` identities; polkitd has already expanded groups into users.
fn user_ids(identities: &[(String, HashMap<String, OwnedValue>)]) -> Vec<u32> {
    identities
        .iter()
        .filter(|(kind, _)| kind == "unix-user")
        .filter_map(|(_, details)| u32::try_from(details.get("uid")?).ok())
        .take(MAX_USERS)
        .collect()
}

async fn read_passwd() -> String {
    let file = match tokio::fs::File::open("/etc/passwd").await {
        Ok(file) => file,
        Err(e) => {
            warn!("polkit: cannot read /etc/passwd: {e}");
            return String::new();
        }
    };
    let mut text = String::new();
    if let Err(e) = file.take(MAX_PASSWD).read_to_string(&mut text).await {
        warn!("polkit: cannot read /etc/passwd: {e}");
    }
    text
}

fn user_name(passwd: &str, uid: u32) -> Option<&str> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let id: u32 = fields.nth(1)?.parse().ok()?;
        (id == uid && !name.is_empty()).then_some(name)
    })
}

fn request(
    action: &str,
    message: &str,
    cookie: &str,
    uids: &[u32],
    passwd: &str,
    current: &str,
) -> Option<Request> {
    let mut users: Vec<String> = Vec::new();
    for name in uids.iter().filter_map(|uid| user_name(passwd, *uid)) {
        let name = clean(name);
        if !users.contains(&name) {
            users.push(name);
        }
    }
    if users.is_empty() {
        return None;
    }
    Some(Request {
        cookie: clean(cookie),
        action: clean(action),
        message: clean(message),
        user: users.iter().position(|u| u == current).unwrap_or(0),
        users,
        prompt: "Password".into(),
        ..Request::default()
    })
}

/// Untrusted text for display or the helper: no control characters, bounded.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

/// Runs authentications one at a time, in the order polkitd asked.
async fn serve(
    mut jobs: mpsc::Receiver<Job>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    state: watch::Sender<Option<Request>>,
) {
    let mut cancelled: Vec<String> = Vec::new();
    while let Some(job) = jobs.recv().await {
        let outcome = if let Some(i) = cancelled.iter().position(|c| *c == job.request.cookie) {
            cancelled.swap_remove(i);
            Err(AgentError::Cancelled("cancelled".into()))
        } else {
            authenticate(job.request, &mut commands, &state, &mut cancelled).await
        };
        state.send_replace(None);
        let _ = job.done.send(outcome); // polkitd may have given up on the call
        if jobs.is_empty() {
            cancelled.clear();
        }
        if commands.is_closed() {
            break;
        }
    }
    warn!("polkit: request channel closed, agent stopping");
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
    /// The response to send at PAM's first prompt.
    pending: Option<Secret>,
}

async fn next_line(session: &mut Option<Session>) -> Option<String> {
    match session {
        Some(session) => session.lines.recv().await,
        None => std::future::pending().await,
    }
}

async fn authenticate(
    mut request: Request,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    state: &watch::Sender<Option<Request>>,
    cancelled: &mut Vec<String>,
) -> Result<(), AgentError> {
    state.send_replace(Some(request.clone()));
    // shortcut: the helper starts on the first submit, so PAM modules that act
    // before a prompt (fingerprint) wait for Enter; start it on open if wanted.
    let mut session: Option<Session> = None;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                None => {
                    warn!("polkit: command channel closed, cancelling");
                    return Err(AgentError::Cancelled("agent stopped".into()));
                }
                Some(Command::Cancel { cookie }) if cookie == request.cookie => {
                    return Err(AgentError::Cancelled("dismissed".into()));
                }
                Some(Command::Cancel { cookie }) => {
                    if cancelled.len() < MAX_QUEUED * 4 {
                        cancelled.push(cookie);
                    }
                }
                Some(Command::Respond { cookie, user, response }) if cookie == request.cookie => {
                    request.error = None;
                    request.info = None;
                    request.busy = true;
                    match session.as_mut() {
                        Some(live) => {
                            if let Err(e) = answer(&mut live.stdin, &response).await {
                                warn!("polkit: helper input: {e}");
                                session = None;
                                fail(&mut request, "The authentication helper stopped.");
                            }
                        }
                        None => {
                            if let Some(name) = request.users.get(user) {
                                request.user = user;
                                match start(name, &request.cookie, response).await {
                                    Ok(live) => session = Some(live),
                                    Err(e) => {
                                        warn!("polkit: {e}");
                                        request.busy = false;
                                        request.error = Some("Couldn't start the authentication helper.".into());
                                    }
                                }
                            } else {
                                request.busy = false;
                            }
                        }
                    }
                    state.send_replace(Some(request.clone()));
                }
                Some(Command::Respond { .. }) => {}
            },
            line = next_line(&mut session) => {
                let Some(live) = session.as_mut() else { continue };
                match line.as_deref().map(parse) {
                    Some(Line::Prompt { text, echo }) => {
                        if let Some(response) = live.pending.take() {
                            if let Err(e) = answer(&mut live.stdin, &response).await {
                                warn!("polkit: helper input: {e}");
                                session = None;
                                fail(&mut request, "The authentication helper stopped.");
                            }
                        } else {
                            request.prompt = text;
                            request.echo = echo;
                            request.busy = false;
                        }
                    }
                    Some(Line::Error(text)) => request.error = Some(text),
                    Some(Line::Info(text)) => request.info = Some(text),
                    Some(Line::Success) => {
                        if let Some(mut live) = session.take()
                            && finish(&mut live.child).await
                        {
                            return Ok(());
                        }
                        fail(&mut request, "Authentication failed.");
                    }
                    Some(Line::Failure) | None => {
                        if let Some(mut live) = session.take() {
                            finish(&mut live.child).await;
                        }
                        let text = match (&line, request.error.take()) {
                            (None, _) => "The authentication helper stopped.".to_owned(),
                            (Some(_), Some(pam)) => pam,
                            (Some(_), None) => "Wrong password. Try again.".to_owned(),
                        };
                        fail(&mut request, &text);
                    }
                    Some(Line::Other) => continue,
                }
                state.send_replace(Some(request.clone()));
            }
        }
    }
}

fn fail(request: &mut Request, text: &str) {
    request.error = Some(text.to_owned());
    request.busy = false;
    request.prompt = "Password".into();
    request.echo = false;
    request.attempts = request.attempts.saturating_add(1);
}

async fn answer(stdin: &mut ChildStdin, response: &Secret) -> std::io::Result<()> {
    // The helper reads one line per prompt; a line break would split the answer.
    let text: String = response.0.chars().filter(|c| *c != '\n').collect();
    stdin.write_all(text.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await
}

/// Waits for the helper to exit and reports whether it succeeded.
async fn finish(child: &mut Child) -> bool {
    match tokio::time::timeout(HELPER_EXIT, child.wait()).await {
        Ok(Ok(status)) if status.success() => true,
        Ok(Ok(status)) => {
            warn!("polkit: polkit-agent-helper-1 exited with {status}");
            false
        }
        Ok(Err(e)) => {
            warn!("polkit: waiting for polkit-agent-helper-1: {e}");
            false
        }
        Err(_) => {
            warn!("polkit: polkit-agent-helper-1 did not exit");
            false
        }
    }
}

async fn start(user: &str, cookie: &str, response: Secret) -> std::io::Result<Session> {
    let helper = HELPERS
        .into_iter()
        .find(|path| Path::new(path).exists())
        .ok_or_else(|| std::io::Error::other("polkit-agent-helper-1 is not installed"))?;
    let mut child = Process::new(helper)
        .arg(user)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| std::io::Error::other(format!("{helper} {user}: {e}")))?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(std::io::Error::other(format!("{helper}: no pipes")));
    };
    stdin.write_all(cookie.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    let (tx, lines) = mpsc::channel(16);
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match (&mut reader).take(MAX_LINE).read_line(&mut line).await {
                Ok(0) => return,
                Ok(_) if !line.ends_with('\n') => {
                    warn!("polkit: helper line too long; stopping");
                    return;
                }
                Ok(_) => {
                    if tx.send(line.trim_end().to_owned()).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    warn!("polkit: reading the helper: {e}");
                    return;
                }
            }
        }
    });
    Ok(Session {
        child,
        stdin,
        lines,
        pending: Some(response),
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Line {
    Prompt { text: String, echo: bool },
    Error(String),
    Info(String),
    Success,
    Failure,
    Other,
}

fn parse(line: &str) -> Line {
    let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
    let text = clean(rest.trim());
    match word {
        "PAM_PROMPT_ECHO_OFF" => Line::Prompt { text, echo: false },
        "PAM_PROMPT_ECHO_ON" => Line::Prompt { text, echo: true },
        "PAM_ERROR_MSG" => Line::Error(text),
        "PAM_TEXT_INFO" => Line::Info(text),
        "SUCCESS" => Line::Success,
        "FAILURE" => Line::Failure,
        _ => Line::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n\
        broken line\n\
        ana:x:1000:1000:Ana:/home/ana:/bin/zsh\n\
        bo:x:1001:1001::/home/bo:/bin/sh\n";

    #[test]
    fn helper_lines_parse() {
        assert_eq!(
            parse("PAM_PROMPT_ECHO_OFF Password: "),
            Line::Prompt {
                text: "Password:".into(),
                echo: false
            }
        );
        assert_eq!(
            parse("PAM_PROMPT_ECHO_ON Login:"),
            Line::Prompt {
                text: "Login:".into(),
                echo: true
            }
        );
        assert_eq!(parse("PAM_ERROR_MSG Locked"), Line::Error("Locked".into()));
        assert_eq!(parse("PAM_TEXT_INFO Touch"), Line::Info("Touch".into()));
        assert_eq!(parse("SUCCESS"), Line::Success);
        assert_eq!(parse("FAILURE"), Line::Failure);
        assert_eq!(parse(""), Line::Other);
    }

    #[test]
    fn request_names_users_and_prefers_the_current_one() {
        let request = request("a.b", "Install?", "c1", &[0, 1000, 1000, 7], PASSWD, "ana").unwrap();
        assert_eq!(request.users, ["root", "ana"]);
        assert_eq!(request.user, 1);
        assert_eq!(request.prompt, "Password");
        let other = super::request("a.b", "m", "c1", &[1001], PASSWD, "ana").unwrap();
        assert_eq!(other.user, 0);
        assert!(super::request("a", "m", "c", &[7, 8], PASSWD, "ana").is_none());
    }

    #[test]
    fn hostile_text_is_stripped_and_bounded() {
        let message = format!("Hi\u{1b}[31m\n{}", "x".repeat(10_000));
        let request = request("a", &message, "c\n", &[0], PASSWD, "").unwrap();
        assert!(request.message.starts_with("Hi[31m"));
        assert_eq!(request.message.chars().count(), MAX_TEXT);
        assert_eq!(request.cookie, "c");
    }

    #[test]
    fn only_unix_users_are_listed_and_bounded() {
        let user = |uid: u32| {
            (
                "unix-user".to_owned(),
                HashMap::from([("uid".to_owned(), OwnedValue::from(uid))]),
            )
        };
        let mut identities = vec![
            (
                "unix-group".to_owned(),
                HashMap::from([("gid".to_owned(), OwnedValue::from(10u32))]),
            ),
            ("unix-user".to_owned(), HashMap::new()),
        ];
        identities.extend((0..100).map(user));
        let uids = user_ids(&identities);
        assert_eq!(uids.len(), MAX_USERS);
        assert_eq!(uids.first(), Some(&0));
    }

    #[test]
    fn secrets_never_print() {
        let secret = Secret::new("hunter2".into());
        assert!(!format!("{secret:?}").contains("hunter2"));
        let command = Command::Respond {
            cookie: "c".into(),
            user: 0,
            response: secret,
        };
        assert!(!format!("{command:?}").contains("hunter2"));
    }

    #[tokio::test]
    async fn cancel_ends_the_current_request() {
        let (state, rx) = watch::channel(None);
        let (commands, mut command_rx) = mpsc::unbounded_channel();
        let request = request("a", "m", "c1", &[0], PASSWD, "").unwrap();
        commands
            .send(Command::Cancel {
                cookie: "other".into(),
            })
            .unwrap();
        commands
            .send(Command::Cancel {
                cookie: "c1".into(),
            })
            .unwrap();
        let mut cancelled = Vec::new();
        let outcome = authenticate(request, &mut command_rx, &state, &mut cancelled).await;
        assert!(matches!(outcome, Err(AgentError::Cancelled(_))));
        assert_eq!(cancelled, ["other"]);
        assert_eq!(rx.borrow().as_ref().map(|r| r.cookie.as_str()), Some("c1"));
    }

    #[tokio::test]
    async fn a_request_cancelled_while_queued_never_shows() {
        let (state, rx) = watch::channel(None);
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (jobs, job_rx) = mpsc::channel(MAX_QUEUED);
        let first = request("a", "m", "c1", &[0], PASSWD, "").unwrap();
        let second = request("a", "m", "c2", &[0], PASSWD, "").unwrap();
        let (done1, out1) = oneshot::channel();
        let (done2, out2) = oneshot::channel();
        jobs.try_send(Job {
            request: first,
            done: done1,
        })
        .unwrap();
        jobs.try_send(Job {
            request: second,
            done: done2,
        })
        .unwrap();
        commands
            .send(Command::Cancel {
                cookie: "c2".into(),
            })
            .unwrap();
        commands
            .send(Command::Cancel {
                cookie: "c1".into(),
            })
            .unwrap();
        drop(jobs);
        serve(job_rx, command_rx, state).await;
        assert!(matches!(out1.await, Ok(Err(AgentError::Cancelled(_)))));
        assert!(matches!(out2.await, Ok(Err(AgentError::Cancelled(_)))));
        assert!(rx.borrow().is_none());
    }
}
