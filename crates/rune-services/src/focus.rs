//! Focus countdowns, measured against Linux boot time, including suspend.

use crate::notifications::{self, Notification, Urgency};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::warn;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Focus,
    Break,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Running,
    Paused,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub status: Status,
    pub kind: Kind,
    pub remaining: u64,
}

impl State {
    pub fn label(&self) -> String {
        format!("{:02}:{:02}", self.remaining / 60, self.remaining % 60)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Start {
        minutes: u32,
        kind: Kind,
        dnd: bool,
    },
    Pause,
    Resume,
    Stop,
    /// An explicit DND change relinquishes the timer's temporary ownership.
    SetDnd(bool),
}

#[derive(Default)]
struct Timer {
    state: State,
    deadline: u64,
    dnd: bool,
    restore_dnd: Option<bool>,
}

impl Timer {
    fn snapshot(&self, now: u64) -> State {
        let mut state = self.state.clone();
        if state.status == Status::Running {
            state.remaining = self.deadline.saturating_sub(now);
        }
        state
    }

    fn stop(&mut self) {
        self.state = State::default();
        if let Some(previous) = self.restore_dnd.take() {
            self.dnd = previous;
        }
    }

    fn tick(&mut self, now: u64) -> bool {
        if self.state.status == Status::Running && now >= self.deadline {
            self.stop();
            true
        } else {
            false
        }
    }

    fn apply(&mut self, command: Command, now: u64) {
        match command {
            Command::Start { minutes, kind, dnd } => {
                if !(1..=1440).contains(&minutes) {
                    return;
                }
                let remaining = u64::from(minutes) * 60;
                let Some(deadline) = now.checked_add(remaining) else {
                    return;
                };
                self.stop();
                self.state = State {
                    status: Status::Running,
                    kind,
                    remaining,
                };
                self.deadline = deadline;
                if dnd && kind == Kind::Focus {
                    self.restore_dnd = Some(self.dnd);
                    self.dnd = true;
                }
            }
            Command::Pause if self.state.status == Status::Running => {
                self.state = self.snapshot(now);
                self.state.status = Status::Paused;
            }
            Command::Resume if self.state.status == Status::Paused => {
                if let Some(deadline) = now.checked_add(self.state.remaining) {
                    self.deadline = deadline;
                    self.state.status = Status::Running;
                }
            }
            Command::Stop => self.stop(),
            Command::SetDnd(on) => {
                self.restore_dnd = None;
                self.dnd = on;
            }
            _ => {}
        }
    }
}

fn boot_seconds() -> std::io::Result<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: time points to a valid writable timespec for the duration of the call.
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    u64::try_from(time.tv_sec).map_err(std::io::Error::other)
}

/// Runs one countdown, independently of UI windows. All explicit DND changes must
/// pass through this command channel so completion respects the user's changes.
pub fn spawn(
    notifications: Option<mpsc::UnboundedSender<notifications::Command>>,
    initial_dnd: bool,
) -> (watch::Receiver<State>, mpsc::UnboundedSender<Command>) {
    let (tx, rx) = watch::channel(State::default());
    let (commands, mut inbox) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut timer = Timer {
            dnd: initial_dnd,
            ..Timer::default()
        };
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let command = tokio::select! {
                command = inbox.recv() => match command {
                    Some(command) => Some(command),
                    None => { warn!("focus: command channel closed; stopping timer"); break; }
                },
                _ = tick.tick() => None,
            };
            let now = match boot_seconds() {
                Ok(now) => now,
                Err(error) => {
                    warn!("focus: reading CLOCK_BOOTTIME failed: {error}");
                    continue;
                }
            };
            let previous_dnd = timer.dnd;
            let kind = timer.state.kind;
            let completed = timer.tick(now);
            let explicit_dnd = matches!(command, Some(Command::SetDnd(_)));
            if let Some(command) = command {
                timer.apply(command, now);
            }
            if let Some(notifications) = &notifications {
                if (previous_dnd != timer.dnd || explicit_dnd)
                    && notifications
                        .send(notifications::Command::SetDnd(timer.dnd))
                        .is_err()
                {
                    warn!("focus: notification channel closed; DND update failed");
                }
                if completed
                    && notifications
                        .send(notifications::Command::Post(Notification {
                            id: 0,
                            app_name: "Focus timer".into(),
                            summary: match kind {
                                Kind::Focus => "Focus complete",
                                Kind::Break => "Break complete",
                            }
                            .into(),
                            body: match kind {
                                Kind::Focus => "Time for a break.",
                                Kind::Break => "Ready for another focus session?",
                            }
                            .into(),
                            image: None,
                            actions: Vec::new(),
                            urgency: Urgency::Normal,
                            time: chrono::Local::now(),
                            resident: false,
                        }))
                        .is_err()
                {
                    warn!("focus: notification channel closed; completion notification failed");
                }
            }
            let state = timer.snapshot(now);
            tx.send_if_modified(|previous| {
                if *previous == state {
                    false
                } else {
                    *previous = state;
                    true
                }
            });
        }
    });
    (rx, commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_resume_and_completion_include_suspend() {
        let mut timer = Timer::default();
        timer.apply(
            Command::Start {
                minutes: 25,
                kind: Kind::Focus,
                dnd: false,
            },
            100,
        );
        assert_eq!(timer.snapshot(110).remaining, 1490);
        timer.apply(Command::Pause, 110);
        assert_eq!(timer.snapshot(900).remaining, 1490);
        timer.apply(Command::Resume, 900);
        assert!(!timer.tick(2389));
        assert!(timer.tick(2390));
        assert!(!timer.tick(2391));
        assert_eq!(timer.snapshot(2391).status, Status::Idle);
    }

    #[test]
    fn bounds_replacement_and_stop() {
        let mut timer = Timer::default();
        for minutes in [0, 1441, u32::MAX] {
            timer.apply(
                Command::Start {
                    minutes,
                    kind: Kind::Focus,
                    dnd: true,
                },
                0,
            );
            assert_eq!(timer.snapshot(0).status, Status::Idle);
        }
        timer.apply(
            Command::Start {
                minutes: 25,
                kind: Kind::Focus,
                dnd: true,
            },
            0,
        );
        assert!(timer.dnd);
        timer.apply(
            Command::Start {
                minutes: 5,
                kind: Kind::Break,
                dnd: false,
            },
            1,
        );
        assert!(!timer.dnd);
        assert_eq!(timer.snapshot(1).remaining, 300);
        timer.apply(Command::Stop, 2);
        assert_eq!(timer.snapshot(2).status, Status::Idle);
    }

    #[test]
    fn extreme_deadline_does_not_start_and_existing_dnd_is_preserved() {
        let mut timer = Timer {
            dnd: true,
            ..Timer::default()
        };
        let start = Command::Start {
            minutes: 1,
            kind: Kind::Focus,
            dnd: true,
        };
        timer.apply(start.clone(), u64::MAX);
        assert_eq!(timer.state.status, Status::Idle);
        timer.apply(start, 0);
        timer.tick(60);
        assert!(timer.dnd);
    }

    #[tokio::test]
    async fn commands_publish_snapshots_and_explicit_dnd_changes() {
        let (notifications, mut posted) = mpsc::unbounded_channel();
        let (mut state, commands) = spawn(Some(notifications), false);
        commands
            .send(Command::Start {
                minutes: 1,
                kind: Kind::Focus,
                dnd: true,
            })
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            state.wait_for(|state| state.status == Status::Running),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(state.borrow_and_update().status, Status::Running);
        assert_eq!(
            posted.recv().await.unwrap(),
            notifications::Command::SetDnd(true)
        );
        commands.send(Command::Pause).unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            state.wait_for(|state| state.status == Status::Paused),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(state.borrow_and_update().status, Status::Paused);
        commands.send(Command::SetDnd(false)).unwrap();
        assert_eq!(
            posted.recv().await.unwrap(),
            notifications::Command::SetDnd(false)
        );
        commands.send(Command::Stop).unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            state.wait_for(|state| state.status == Status::Idle),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(state.borrow_and_update().status, Status::Idle);
        assert!(posted.try_recv().is_err());
    }

    #[test]
    fn dnd_restores_only_while_owned() {
        let mut timer = Timer::default();
        let start = Command::Start {
            minutes: 1,
            kind: Kind::Focus,
            dnd: true,
        };
        timer.apply(start.clone(), 0);
        assert!(timer.dnd);
        assert!(timer.tick(60));
        assert!(!timer.dnd);
        timer.apply(start, 100);
        timer.apply(Command::SetDnd(true), 110);
        timer.apply(Command::Stop, 120);
        assert!(timer.dnd);
    }
}
