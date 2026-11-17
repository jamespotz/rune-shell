//! System-wide 10-band equalizer: a PipeWire filter-chain run as a child
//! `pipewire -c` process. Its sink becomes the default output while it is on;
//! band gains change live through `pw-cli set-param`.

use std::{process::Stdio, time::Duration};
use tokio::{
    process::{Child, Command as Process},
    sync::{mpsc, oneshot, watch},
};
use tracing::warn;

pub const SINK: &str = "rune_equalizer";
const PLAYBACK: &str = "rune_equalizer.output";
pub const BAND_HZ: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
/// Band gain in dB, 31 Hz first.
pub type Gains = [f32; 10];
pub const MAX_DB: f32 = 12.0;

pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub gains: Gains,
}

pub const PRESETS: [Preset; 9] = [
    preset("flat", "Flat", [0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]),
    preset(
        "bass",
        "Bass Boost",
        [6., 5., 4., 2., 0., 0., 0., 0., 0., 0.],
    ),
    preset(
        "treble",
        "Treble Boost",
        [0., 0., 0., 0., 0., 1., 2., 4., 5., 6.],
    ),
    preset(
        "vocal",
        "Vocal",
        [-2., -2., -1., 1., 3., 4., 3., 1., 0., -1.],
    ),
    preset("rock", "Rock", [4., 3., 2., 0., -1., -1., 1., 2., 3., 4.]),
    preset("pop", "Pop", [-1., 0., 2., 3., 4., 3., 1., 0., -1., -1.]),
    preset(
        "electronic",
        "Electronic",
        [5., 4., 1., 0., -2., 1., 0., 2., 4., 5.],
    ),
    preset(
        "acoustic",
        "Acoustic",
        [3., 3., 2., 1., 1., 1., 2., 3., 2., 1.],
    ),
    preset(
        "night",
        "Late Night",
        [4., 3., 1., 0., -1., -1., 0., 1., 2., 3.],
    ),
];

const fn preset(id: &'static str, label: &'static str, gains: Gains) -> Preset {
    Preset { id, label, gains }
}

/// The preset whose gains these are, if any.
pub fn preset_of(gains: &Gains) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.gains == *gains)
}

/// A gain safe to hand to PipeWire.
pub fn clamp_gain(db: f32) -> f32 {
    if db.is_finite() {
        db.clamp(-MAX_DB, MAX_DB)
    } else {
        0.
    }
}

/// Attenuation that keeps the loudest boost from clipping.
fn preamp(gains: &Gains) -> f32 {
    let boost = gains.iter().copied().map(clamp_gain).fold(0., f32::max);
    10f32.powf(-boost / 20.)
}

fn band(index: usize) -> (&'static str, f32) {
    match index {
        0 => ("bq_lowshelf", 0.7),
        9 => ("bq_highshelf", 0.7),
        _ => ("bq_peaking", 1.0),
    }
}

/// Sink names are interpolated into the SPA config, so only plain ones pass.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:@+".contains(c))
}

/// The `pipewire -c` config for a chain playing into `output`.
pub fn chain_conf(gains: &Gains, output: &str) -> Option<String> {
    if !plain_name(output) {
        return None;
    }
    let pre = format!(
        "{{ type = builtin name = pre label = linear control = {{ \"Mult\" = {:.6} \"Add\" = 0.0 }} }}\n",
        preamp(gains)
    );
    let bands: String = BAND_HZ
        .iter()
        .zip(gains)
        .enumerate()
        .map(|(index, (hz, gain))| {
            let (label, q) = band(index);
            format!(
                "{{ type = builtin name = b{} label = {label} control = {{ \"Freq\" = {hz}.0 \"Q\" = {q:.1} \"Gain\" = {:.2} }} }}\n",
                index + 1,
                clamp_gain(*gain)
            )
        })
        .collect();
    let nodes = pre + &bands;
    let links: String = std::iter::once("{ output = \"pre:Out\" input = \"b1:In\" }\n".to_owned())
        .chain(
            (2..=BAND_HZ.len())
                .map(|n| format!("{{ output = \"b{}:Out\" input = \"b{n}:In\" }}\n", n - 1)),
        )
        .collect();
    Some(format!(
        r#"context.properties = {{ log.level = 0 }}
context.spa-libs = {{ audio.convert.* = audioconvert/libspa-audioconvert support.* = support/libspa-support }}
context.modules = [
  {{ name = libpipewire-module-rt flags = [ ifexists nofail ] }}
  {{ name = libpipewire-module-protocol-native }}
  {{ name = libpipewire-module-client-node }}
  {{ name = libpipewire-module-adapter }}
  {{ name = libpipewire-module-filter-chain
    args = {{
      node.description = "Rune Equalizer"
      media.name = "Rune Equalizer"
      filter.graph = {{
        nodes = [
{nodes}        ]
        links = [
{links}        ]
      }}
      audio.channels = 2
      audio.position = [ FL FR ]
      capture.props = {{ node.name = "{SINK}" media.class = Audio/Sink }}
      playback.props = {{ node.name = "{PLAYBACK}" node.passive = true target.object = "{output}" }}
    }}
  }}
]
"#
    ))
}

/// The `Props` pod `pw-cli set-param` takes for these gains.
pub fn props(gains: &Gains) -> String {
    let bands: String = gains
        .iter()
        .enumerate()
        .map(|(index, gain)| format!(" \"b{}:Gain\" {:.2}", index + 1, clamp_gain(*gain)))
        .collect();
    let params = format!("\"pre:Mult\" {:.6}{bands}", preamp(gains));
    format!("{{ params = [ {params} ] }}")
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Enable,
    Disable,
    Gains(Gains),
    /// Send the equalized sound to this output instead.
    Retarget(String),
}

/// The audio programs the equalizer drives. One adapter runs the real ones;
/// tests use an in-memory one.
pub trait Runner: Send {
    fn default_sink(&mut self) -> impl Future<Output = Result<String, String>> + Send;
    fn set_default_sink(&mut self, name: &str) -> impl Future<Output = Result<(), String>> + Send;
    /// Starts the chain and waits until its sink exists.
    fn start(&mut self, conf: String) -> impl Future<Output = Result<(), String>> + Send;
    fn set_props(&mut self, props: &str) -> impl Future<Output = Result<(), String>> + Send;
    fn retarget(&mut self, output: &str) -> impl Future<Output = Result<(), String>> + Send;
    /// Stops the chain and waits for it to exit.
    fn stop(&mut self) -> impl Future<Output = ()> + Send;
    /// Resolves when a running chain exits by itself; pending otherwise.
    fn exited(&mut self) -> impl Future<Output = ()> + Send;
}

enum Request {
    Command(Command),
    /// Turn off, acknowledge, and end the service.
    Stop(oneshot::Sender<()>),
}

pub struct Handle {
    requests: mpsc::UnboundedSender<Request>,
    gains: watch::Sender<Gains>,
    output: watch::Receiver<Option<String>>,
}

impl Handle {
    pub fn send(&self, command: Command) {
        match command {
            Command::Gains(gains) => {
                self.gains
                    .send_if_modified(|g| std::mem::replace(g, gains) != gains);
            }
            command => {
                if self.requests.send(Request::Command(command)).is_err() {
                    warn!("equalizer: service stopped; command dropped");
                }
            }
        }
    }

    /// The real output the chain plays into; `None` while the equalizer is off.
    pub fn output(&self) -> watch::Receiver<Option<String>> {
        self.output.clone()
    }

    /// Turns the equalizer off (restoring the default output) and waits for it.
    pub async fn shutdown(&self) {
        let (done, wait) = oneshot::channel();
        if self.requests.send(Request::Stop(done)).is_ok() {
            // An error means the service already ended, which is also done.
            let _ = wait.await;
        }
    }
}

pub fn spawn(gains: Gains) -> Handle {
    spawn_with(Pipewire::default(), gains)
}

fn spawn_with(runner: impl Runner + 'static, gains: Gains) -> Handle {
    let (requests, rx) = mpsc::unbounded_channel();
    let (gains, gains_rx) = watch::channel(gains);
    let (output_tx, output) = watch::channel(None);
    tokio::spawn(run(runner, rx, gains_rx, output_tx));
    Handle {
        requests,
        gains,
        output,
    }
}

const RESTARTS: u32 = 3;

async fn run(
    mut runner: impl Runner,
    mut requests: mpsc::UnboundedReceiver<Request>,
    mut gains: watch::Receiver<Gains>,
    output: watch::Sender<Option<String>>,
) {
    let mut wanted = false;
    let mut crashes = 0;
    loop {
        let active = output.borrow().is_some();
        tokio::select! {
            request = requests.recv() => match request {
                None => {
                    deactivate(&mut runner, &output).await;
                    return;
                }
                Some(Request::Stop(done)) => {
                    deactivate(&mut runner, &output).await;
                    let _ = done.send(());
                    return;
                }
                Some(Request::Command(Command::Enable)) => {
                    wanted = true;
                    crashes = 0;
                    if !active {
                        activate(&mut runner, &mut gains, &output).await;
                    }
                }
                Some(Request::Command(Command::Disable)) => {
                    wanted = false;
                    deactivate(&mut runner, &output).await;
                }
                Some(Request::Command(Command::Retarget(sink))) if active => {
                    match runner.retarget(&sink).await {
                        Ok(()) => {
                            output.send_replace(Some(sink));
                        }
                        Err(e) => warn!("equalizer: switching output to {sink}: {e}"),
                    }
                }
                Some(Request::Command(Command::Retarget(_) | Command::Gains(_))) => {}
            },
            Ok(()) = gains.changed(), if active => {
                let props = props(&gains.borrow_and_update());
                if let Err(e) = runner.set_props(&props).await {
                    warn!("equalizer: setting gains: {e}");
                }
            }
            () = runner.exited(), if active => {
                warn!("equalizer: filter chain exited");
                deactivate(&mut runner, &output).await;
                if wanted && crashes < RESTARTS {
                    tokio::time::sleep(Duration::from_secs(1 << crashes)).await;
                    crashes += 1;
                    activate(&mut runner, &mut gains, &output).await;
                } else if wanted {
                    warn!("equalizer: giving up after {RESTARTS} restarts");
                }
            }
        }
    }
}

async fn activate(
    runner: &mut impl Runner,
    gains: &mut watch::Receiver<Gains>,
    output: &watch::Sender<Option<String>>,
) {
    let previous = match runner.default_sink().await {
        Ok(sink) if sink != SINK => sink,
        Ok(_) => {
            warn!("equalizer: default output is already {SINK}; not stacking another");
            return;
        }
        Err(e) => {
            warn!("equalizer: reading the default output: {e}");
            return;
        }
    };
    let Some(conf) = chain_conf(&gains.borrow_and_update(), &previous) else {
        warn!("equalizer: output name {previous:?} is not safe to use");
        return;
    };
    if let Err(e) = runner.start(conf).await {
        warn!("equalizer: starting the filter chain: {e}");
        runner.stop().await;
        return;
    }
    if let Err(e) = runner.set_default_sink(SINK).await {
        warn!("equalizer: making it the default output: {e}");
        runner.stop().await;
        return;
    }
    output.send_replace(Some(previous));
}

async fn deactivate(runner: &mut impl Runner, output: &watch::Sender<Option<String>>) {
    let Some(previous) = output.send_replace(None) else {
        return;
    };
    // Move the sound back before the chain disappears.
    if let Err(e) = runner.set_default_sink(&previous).await {
        warn!("equalizer: restoring the default output {previous}: {e}");
    }
    runner.stop().await;
}

/// The real programs: `pipewire`, `pw-dump`, `pw-cli`, `pw-metadata`, `pactl`.
#[derive(Default)]
struct Pipewire {
    child: Option<Child>,
    sink_id: Option<u64>,
}

async fn run_checked(program: &str, args: &[&str]) -> Result<String, String> {
    let out = Process::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{program} {}: {}", args.join(" "), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Node id of `name` in `pw-dump` output.
fn node_id(dump: &str, name: &str) -> Option<u64> {
    let objects: Vec<serde_json::Value> = serde_json::from_str(dump).ok()?;
    objects.iter().find_map(|o| {
        (o.pointer("/info/props/node.name")?.as_str()? == name).then(|| o.get("id")?.as_u64())?
    })
}

async fn find_node(name: &str) -> Result<Option<u64>, String> {
    Ok(node_id(&run_checked("pw-dump", &[]).await?, name))
}

impl Runner for Pipewire {
    async fn default_sink(&mut self) -> Result<String, String> {
        run_checked("pactl", &["get-default-sink"]).await
    }

    async fn set_default_sink(&mut self, name: &str) -> Result<(), String> {
        run_checked("pactl", &["set-default-sink", name])
            .await
            .map(drop)
    }

    async fn start(&mut self, conf: String) -> Result<(), String> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(|d| std::path::PathBuf::from(d).join("rune-shell"))
            .ok_or("XDG_RUNTIME_DIR is not set")?;
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let path = dir.join("equalizer.conf");
        let temp = dir.join("equalizer.conf.tmp");
        tokio::fs::write(&temp, conf)
            .await
            .map_err(|e| format!("writing {}: {e}", temp.display()))?;
        tokio::fs::rename(&temp, &path)
            .await
            .map_err(|e| format!("replacing {}: {e}", path.display()))?;
        let child = Process::new("pipewire")
            .arg("-c")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("pipewire -c {}: {e}", path.display()))?;
        let child = self.child.insert(child);
        for _ in 0..30 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("pipewire -c {} exited: {status}", path.display()));
            }
            if let Some(id) = find_node(SINK).await? {
                self.sink_id = Some(id);
                return Ok(());
            }
        }
        Err(format!("{SINK} did not appear within 3 s"))
    }

    async fn set_props(&mut self, props: &str) -> Result<(), String> {
        let id = self.sink_id.ok_or("the filter chain is not running")?;
        run_checked("pw-cli", &["set-param", &id.to_string(), "Props", props])
            .await
            .map(drop)
    }

    async fn retarget(&mut self, output: &str) -> Result<(), String> {
        if !plain_name(output) {
            return Err(format!("output name {output:?} is not safe to use"));
        }
        let id = find_node(PLAYBACK)
            .await?
            .ok_or(format!("{PLAYBACK} is not running"))?;
        run_checked("pw-metadata", &[&id.to_string(), "target.object", output])
            .await
            .map(drop)
    }

    async fn stop(&mut self) {
        self.sink_id = None;
        if let Some(mut child) = self.child.take()
            && let Err(e) = child.kill().await
        {
            warn!("equalizer: stopping pipewire: {e}");
        }
    }

    async fn exited(&mut self) {
        match &mut self.child {
            Some(child) => {
                let status = child.wait().await;
                warn!("equalizer: pipewire exited: {status:?}");
                self.child = None;
                self.sink_id = None;
            }
            None => std::future::pending().await,
        }
    }
}

/// The output a "make this the default output" request should retarget the
/// equalizer to, when the equalizer is on.
pub fn retarget_for(command: &crate::audio::Command, active: bool) -> Option<&str> {
    match command {
        crate::audio::Command::Device(crate::audio::DeviceCommand::SetDefault(true, name))
            if active && name != SINK =>
        {
            Some(name)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn chain_conf_builds_ten_bands_with_headroom() {
        let gains = PRESETS[1].gains; // bass, +6 dB peak
        let conf = chain_conf(&gains, "alsa_output.pci-0000_00_1f.3.analog-stereo").unwrap();
        for n in 1..=10 {
            assert!(conf.contains(&format!("name = b{n} ")), "band {n}");
        }
        assert!(conf.contains("label = bq_lowshelf control = { \"Freq\" = 31.0"));
        assert!(conf.contains("label = bq_highshelf control = { \"Freq\" = 16000.0"));
        assert!(conf.contains("\"Mult\" = 0.501187"));
        assert!(conf.contains("{ output = \"b9:Out\" input = \"b10:In\" }"));
        assert!(conf.contains("node.name = \"rune_equalizer\" media.class = Audio/Sink"));
        assert!(conf.contains("target.object = \"alsa_output.pci-0000_00_1f.3.analog-stereo\""));
    }

    #[test]
    fn hostile_output_names_and_gains_are_refused_or_clamped() {
        let gains = [0.; 10];
        for name in ["", "a\"b", "a\nb", "a } b", &"x".repeat(300)] {
            assert_eq!(chain_conf(&gains, name), None, "{name:?}");
        }
        let wild = [f32::NAN, f32::INFINITY, 1e9, -1e9, 0., 0., 0., 0., 0., 0.];
        let p = props(&wild);
        assert!(p.contains("\"b1:Gain\" 0.00"));
        assert!(p.contains("\"b3:Gain\" 12.00"));
        assert!(p.contains("\"b4:Gain\" -12.00"));
        assert!(p.contains("\"pre:Mult\" 0.251189"));
    }

    #[test]
    fn presets_are_found_by_their_gains() {
        for p in &PRESETS {
            assert_eq!(preset_of(&p.gains).map(|f| f.id), Some(p.id));
        }
        assert!(preset_of(&[1.; 10]).is_none());
    }

    #[test]
    fn finds_nodes_in_pw_dump() {
        let dump = r#"[{"id":5,"info":{"props":{"node.name":"other"}}},
            {"id":110,"info":{"props":{"node.name":"rune_equalizer"}}},
            {"id":7,"type":"PipeWire:Interface:Core"}]"#;
        assert_eq!(node_id(dump, SINK), Some(110));
        assert_eq!(node_id(dump, PLAYBACK), None);
        assert_eq!(node_id("not json", SINK), None);
    }

    #[test]
    fn only_output_switches_retarget_while_on() {
        use crate::audio::{Command as Audio, DeviceCommand};
        let out = Audio::Device(DeviceCommand::SetDefault(true, "speakers".into()));
        assert_eq!(retarget_for(&out, true), Some("speakers"));
        assert_eq!(retarget_for(&out, false), None);
        let mic = Audio::Device(DeviceCommand::SetDefault(false, "mic".into()));
        assert_eq!(retarget_for(&mic, true), None);
    }

    #[tokio::test]
    #[ignore = "starts a real filter chain on the session's PipeWire"]
    async fn real_pipewire_round_trip() {
        let mut pw = Pipewire::default();
        let output = pw.default_sink().await.unwrap();
        pw.start(chain_conf(&PRESETS[1].gains, &output).unwrap())
            .await
            .unwrap();
        pw.set_props(&props(&PRESETS[2].gains)).await.unwrap();
        let dump = run_checked("pw-dump", &[&pw.sink_id.unwrap().to_string()])
            .await
            .unwrap();
        let objects: Vec<serde_json::Value> = serde_json::from_str(&dump).unwrap();
        let gain = objects[0]["info"]["params"]["Props"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["params"].as_array())
            .find_map(|list| {
                let at = list.iter().position(|v| v == "b10:Gain")?;
                list.get(at + 1)?.as_f64()
            });
        assert_eq!(gain, Some(6.0));
        pw.retarget(&output).await.unwrap();
        pw.stop().await;
        assert_eq!(find_node(SINK).await.unwrap(), None);
        assert_eq!(pw.default_sink().await.unwrap(), output);
    }

    #[derive(Default)]
    struct Log {
        default: String,
        calls: Vec<String>,
        fail_start: bool,
        running: bool,
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Log>>, Arc<tokio::sync::Notify>);

    impl Fake {
        fn with_default(name: &str) -> Self {
            let fake = Self::default();
            fake.log().default = name.into();
            fake
        }
        fn log(&self) -> std::sync::MutexGuard<'_, Log> {
            self.0.lock().unwrap()
        }
        fn calls(&self) -> Vec<String> {
            self.log().calls.clone()
        }
    }

    impl Runner for Fake {
        async fn default_sink(&mut self) -> Result<String, String> {
            Ok(self.log().default.clone())
        }
        async fn set_default_sink(&mut self, name: &str) -> Result<(), String> {
            let mut log = self.log();
            log.default = name.into();
            log.calls.push(format!("default {name}"));
            Ok(())
        }
        async fn start(&mut self, _: String) -> Result<(), String> {
            let mut log = self.log();
            log.calls.push("start".into());
            if log.fail_start {
                return Err("no pipewire".into());
            }
            log.running = true;
            Ok(())
        }
        async fn set_props(&mut self, _: &str) -> Result<(), String> {
            self.log().calls.push("props".into());
            Ok(())
        }
        async fn retarget(&mut self, output: &str) -> Result<(), String> {
            self.log().calls.push(format!("retarget {output}"));
            Ok(())
        }
        async fn stop(&mut self) {
            let mut log = self.log();
            log.running = false;
            log.calls.push("stop".into());
        }
        async fn exited(&mut self) {
            if !self.log().running {
                return std::future::pending().await;
            }
            self.1.notified().await;
            self.log().running = false;
        }
    }

    async fn settle() {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn enable_then_disable_restores_the_output() {
        let fake = Fake::with_default("speakers");
        let handle = spawn_with(fake.clone(), [0.; 10]);
        handle.send(Command::Enable);
        settle().await;
        assert_eq!(fake.log().default, SINK);
        assert_eq!(*handle.output().borrow(), Some("speakers".into()));
        handle.send(Command::Disable);
        settle().await;
        assert_eq!(fake.log().default, "speakers");
        assert_eq!(
            fake.calls(),
            [
                "start",
                "default rune_equalizer",
                "default speakers",
                "stop"
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_restores_the_output() {
        let fake = Fake::with_default("speakers");
        let handle = spawn_with(fake.clone(), [0.; 10]);
        handle.send(Command::Enable);
        settle().await;
        handle.shutdown().await;
        assert_eq!(fake.log().default, "speakers");
        assert_eq!(fake.calls().last().map(String::as_str), Some("stop"));
    }

    #[tokio::test(start_paused = true)]
    async fn failed_start_leaves_the_default_alone() {
        let fake = Fake::with_default("speakers");
        fake.log().fail_start = true;
        let handle = spawn_with(fake.clone(), [0.; 10]);
        handle.send(Command::Enable);
        settle().await;
        assert_eq!(fake.log().default, "speakers");
        assert_eq!(*handle.output().borrow(), None);
        assert_eq!(fake.calls(), ["start", "stop"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_crash_restores_the_output_then_restarts() {
        let fake = Fake::with_default("speakers");
        let handle = spawn_with(fake.clone(), [0.; 10]);
        handle.send(Command::Enable);
        settle().await;
        fake.1.notify_one();
        settle().await;
        assert_eq!(fake.log().default, "speakers");
        tokio::time::sleep(Duration::from_secs(2)).await;
        settle().await;
        assert_eq!(fake.log().default, SINK);
        assert_eq!(fake.calls().iter().filter(|c| *c == "start").count(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_of_gain_changes_is_coalesced() {
        let fake = Fake::with_default("speakers");
        let handle = spawn_with(fake.clone(), [0.; 10]);
        handle.send(Command::Enable);
        settle().await;
        for step in 0..100 {
            handle.send(Command::Gains([step as f32 / 10.; 10]));
        }
        settle().await;
        let props = fake.calls().iter().filter(|c| *c == "props").count();
        assert!((1..=2).contains(&props), "{props} pw-cli calls");
    }

    #[tokio::test(start_paused = true)]
    async fn retargeting_follows_output_choice_only_while_on() {
        let fake = Fake::with_default("speakers");
        let handle = spawn_with(fake.clone(), [0.; 10]);
        handle.send(Command::Retarget("headphones".into()));
        settle().await;
        assert!(fake.calls().is_empty());
        handle.send(Command::Enable);
        handle.send(Command::Retarget("headphones".into()));
        settle().await;
        assert_eq!(*handle.output().borrow(), Some("headphones".into()));
        handle.send(Command::Disable);
        settle().await;
        assert_eq!(fake.log().default, "headphones");
    }
}
