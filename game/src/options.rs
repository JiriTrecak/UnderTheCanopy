//! Command-line options.
//!
//! ```text
//! canopy [--data <folder>] [--no-vsync] [--map <id> | --spectate <id>] [--players <n>] [--races <race,…>]
//!        [--tick <n>] [--camera <x,z,zoom[,yaw]>] [--hour <h>] [graphics]
//! canopy edit [--data <folder>]  (opens the editor; docs/editor.md)
//! canopy mcp [--data <folder>]   (the editor's MCP server without a window)
//! canopy replay <file.utcr> [--verify] [--data <folder>]  (watch a replay; --verify plays it headless and checks it)
//! canopy replay <out.utcr> --record <map> [--ticks <n>]  (records a stand-in match: a computer player's commands sent as a human's)
//! canopy capture-screens --out <folder> [--size <w>x<h>] [--scale <s>] [--only releases|multiplayer|editor|blocks|neutrals|settings|feedback|replays]
//!                         [--data <folder>]
//! canopy shot --map <id> --camera <x,z,zoom[,yaw] | battle[,zoom]> --out <png>
//!             [--tick <n>] [--players <n>] [--races <race,…>] [--crowd <n> | --gallery [--only <text>]] [--colors <i,j,…>] [--size <w>x<h>]
//!             [--hour <h>] [--time <s>] [--frames <n>] [--no-shadows] [--pitch <deg>]
//!             [--movers <n>] [--sequence <frames>,<step>] [--effect <id>[,<age>[,<radius>]]]…
//!             [--flight <id>,<age>,<metres>,<ticks>]…
//!             [--kill <text>[,<ticks>]] [--viewer <slot>]
//!             [--select all|player:<slot>|<id,…>] [--health damaged|always|selected]
//!             [--damage <percent>[,<percent>…]] [--data <folder>] [graphics]
//!             [--play | --observe] [--input <script>] (the HUD; see `play::capture`)
//! graphics: [--preset low|medium|high|ultra] (first; later options adjust it)
//!           [--render-scale <0.5–1>] [--no-contact] [--no-bloom] [--haze off|low|medium|high]
//!           [--lod-pixels <px>] (live models' level-of-detail deviation; 0: full detail)
//!           [--shadow-map <256–4096>]
//!           [--biome <id>] (preview the map in another biome's look)
//! ```
//!
//! `--map` plays a skirmish (you on the first start, computer players on the others, or
//! on the first `--players` starts); `--spectate` watches computer players on every
//! start (or the first `--players`); `--tick` fast-forwards the match first. A shot's `battle`
//! camera looks at the largest fight of that tick. `--crowd <n>` replaces the match by a
//! measured battle of eight players with `n` soldiers each; `--colors i,j,…` gives slot
//! `k` the `k`-th listed player colour (to review every colour).

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

/// A camera pose: focus `x`, `z` (cells), zoom (1 = the game's default distance) and yaw
/// in degrees (0 looks north, as in the game).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSpec {
    pub x: f32,
    pub z: f32,
    pub zoom: f32,
    pub yaw: f32,
}

impl CameraSpec {
    pub fn parse(text: &str) -> Result<Self> {
        let parts: Vec<f32> = text
            .split(',')
            .map(|p| p.trim().parse::<f32>())
            .collect::<Result<_, _>>()
            .with_context(|| format!("--camera {text}: expected x,z,zoom[,yaw]"))?;
        match parts[..] {
            [x, z, zoom] => Ok(Self {
                x,
                z,
                zoom,
                yaw: 0.0,
            }),
            [x, z, zoom, yaw] => Ok(Self { x, z, zoom, yaw }),
            _ => bail!("--camera {text}: expected x,z,zoom[,yaw]"),
        }
    }
}

/// Where a shot looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraChoice {
    Pose(CameraSpec),
    /// The largest fight at the shot's tick, at this zoom.
    Battle(f32),
}

impl CameraChoice {
    pub fn parse(text: &str) -> Result<Self> {
        if let Some(rest) = text.strip_prefix("battle") {
            let zoom = match rest.strip_prefix(',') {
                Some(z) => z.trim().parse().context("--camera battle,<zoom>")?,
                None if rest.is_empty() => 1.0,
                None => bail!("--camera {text}: expected battle[,zoom]"),
            };
            return Ok(Self::Battle(zoom));
        }
        CameraSpec::parse(text).map(Self::Pose)
    }

    pub fn pose(self) -> Option<CameraSpec> {
        match self {
            Self::Pose(p) => Some(p),
            Self::Battle(_) => None,
        }
    }
}

/// A standalone effect a shot plays at the camera's focus: its id, how many ticks before
/// the captured tick it started, and the radius its sizes take (`sizeFrom: radius`).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectSpec {
    pub id: String,
    pub age: u32,
    pub radius: Option<f32>,
    /// `--flight`: a weapon launch effect flown over `(metres, ticks)` towards the focus,
    /// as a shooter's would be.
    pub flight: Option<(f32, u32)>,
}

impl EffectSpec {
    pub fn parse(text: &str) -> Result<Self> {
        let mut parts = text.split(',').map(str::trim);
        let id = parts
            .next()
            .filter(|s| !s.is_empty())
            .context("--effect <id>")?;
        let age = match parts.next() {
            Some(age) => age.parse().context("--effect <id>,<age ticks>")?,
            None => 10,
        };
        let radius = parts
            .next()
            .map(|r| r.parse().context("--effect <id>,<age>,<radius>"))
            .transpose()?;
        Ok(Self {
            id: id.to_owned(),
            age,
            radius,
            flight: None,
        })
    }

    /// `--flight <id>,<age>,<metres>,<ticks>`.
    pub fn parse_flight(text: &str) -> Result<Self> {
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        let [id, age, metres, ticks] = parts[..] else {
            bail!("--flight <id>,<age ticks>,<metres>,<flight ticks>");
        };
        Ok(Self {
            id: id.to_owned(),
            age: age.parse().context("--flight age")?,
            radius: None,
            flight: Some((
                metres.parse().context("--flight metres")?,
                ticks.parse().context("--flight ticks")?,
            )),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Open a window.
    Play,
    /// Open a window in the editor (`canopy edit`).
    Edit,
    /// The editor's MCP server without a window (`canopy mcp`).
    Mcp,
    /// Render offscreen and write a PNG.
    Shot { out: PathBuf, size: [u32; 2] },
    /// Watch a replay (`canopy replay <file>`), or check it headless (`--verify`).
    Replay {
        file: PathBuf,
        verify: bool,
        /// `--record <map> [--ticks n]`: writes a stand-in match to `file` (developers).
        record: Option<(String, u32)>,
    },
    /// Render every screen offscreen into a folder.
    CaptureScreens {
        out: PathBuf,
        size: [u32; 2],
        scale: f32,
        /// One group of captures only (`releases`, `multiplayer`, `editor`, `settings`, `feedback`).
        only: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub command: Command,
    /// Game data folder; overrides the bundled location (`just play` passes the repository).
    pub data: Option<PathBuf>,
    /// Present without waiting for vertical sync, for measuring frame cost.
    pub vsync: bool,
    /// Map id (file name without extension).
    pub map: Option<String>,
    /// Watch computer players instead of playing.
    pub spectate: bool,
    /// Computer players on the map's first starts (all by default).
    pub players: Option<usize>,
    /// Simulate to this tick before showing anything.
    pub tick: u32,
    /// A measured battle: this many soldiers for each of eight players.
    pub crowd: Option<u32>,
    /// One of every unit and building instead of a match (review captures).
    pub gallery: bool,
    /// The gallery shows only definitions whose id contains this (`--only beetles`).
    pub only: Option<String>,
    /// Player colour index of each slot (`config/players.json`).
    pub colors: Option<Vec<usize>>,
    /// Race of each slot in order (`--races beetles,ants`); the start's own when absent.
    pub races: Option<Vec<String>>,
    pub camera: Option<CameraChoice>,
    /// Hour of the day; the map's own when absent.
    pub hour: Option<f32>,
    /// Weather (`clear`, `rain`, `snow`, `godRays`); the map's own when absent.
    pub weather: Option<String>,
    /// Animation time of a shot, seconds (wind, canopy drift).
    pub time: f32,
    /// Frames a shot renders before the captured one (for timings).
    pub frames: u32,
    pub shadows: bool,
    /// Graphics quality (render scale, post effects).
    pub quality: canopy_render::Quality,
    /// Camera pitch of a shot, degrees below the horizon (the game's 56 by default).
    pub pitch: Option<f32>,
    /// Synthetic bodies a shot walks through the view before capturing (ripples).
    pub movers: u32,
    /// `--sequence <frames>,<step>`: after the shot, `frames − 1` more pictures, each `step`
    /// ticks later (`<out>-001.png`, …): effects and fights as animations.
    pub sequence: Option<(u32, u32)>,
    /// Another biome's look for the map (previews).
    pub biome: Option<String>,
    /// Standalone effects a shot plays at the camera's focus, in a row.
    pub effects: Vec<EffectSpec>,
    /// Watch as this player (0-based slot): their fog of war. Observers see everything.
    pub viewer: Option<u8>,
    /// What a shot selects for the world UI.
    pub select: Selection,
    /// Which health bars show.
    pub health: canopy_scene::world_ui::HealthBars,
    /// A dev tool for reviewing damage effects: before a shot's tick, every built building
    /// is set to these shares of its health (percent, in turn by entity id).
    pub damage: Vec<f32>,
    /// A dev tool for reviewing deaths: `ticks` before a shot's tick, every unit whose
    /// definition id contains the text dies (its death clip and its effects).
    pub kill: Option<(String, u32)>,
    /// A shot of the match with the HUD from player 1's seat (`--play`) or the
    /// spectator's (`--observe`), after the `--input` script (`play::capture`).
    pub play: bool,
    pub observe: bool,
    pub input: String,
}

/// What a shot selects (world UI reviews).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Selection {
    #[default]
    None,
    /// Everything in the presentation.
    All,
    /// The units and buildings of one player slot.
    Player(u8),
    Ids(Vec<u32>),
}

impl Selection {
    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "none" => Self::None,
            "all" => Self::All,
            _ => match text.strip_prefix("player:") {
                Some(slot) => Self::Player(slot.parse().context("--select player:<slot>")?),
                None => Self::Ids(
                    text.split(',')
                        .map(|id| id.trim().parse::<u32>())
                        .collect::<Result<_, _>>()
                        .context("--select all | player:<slot> | <id,id,…>")?,
                ),
            },
        })
    }
}

impl Options {
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self {
            command: Command::Play,
            data: None,
            vsync: true,
            map: None,
            spectate: false,
            players: None,
            races: None,
            tick: 0,
            crowd: None,
            gallery: false,
            only: None,
            colors: None,
            camera: None,
            hour: None,
            weather: None,
            time: 12.0,
            frames: 1,
            shadows: true,
            quality: canopy_render::Quality::default(),
            pitch: None,
            movers: 0,
            sequence: None,
            biome: None,
            effects: Vec::new(),
            damage: Vec::new(),
            kill: None,
            viewer: None,
            select: Selection::None,
            health: canopy_scene::world_ui::HealthBars::Damaged,
            play: false,
            observe: false,
            input: String::new(),
        };
        let mut out = None;
        let mut size = [1920, 1080];
        let mut shot = false;
        let mut capture = false;
        let mut scale = 1.0f32;
        let mut only = None;
        let mut first = true;
        let mut replay = false;
        let mut replay_file: Option<PathBuf> = None;
        let mut verify = false;
        let mut record: Option<String> = None;
        let mut record_ticks = 48_000u32;
        while let Some(arg) = args.next() {
            let mut value =
                |name: &str| args.next().with_context(|| format!("{name} needs a value"));
            match arg.as_str() {
                "shot" if first => shot = true,
                "capture-screens" if first => capture = true,
                "--scale" => scale = value("--scale")?.parse().context("--scale")?,
                "--only" if capture => only = Some(value("--only")?),
                "play" if first => {}
                "replay" if first => replay = true,
                "--verify" if replay => verify = true,
                "--record" if replay => record = Some(value("--record")?),
                "--ticks" if replay => {
                    record_ticks = value("--ticks")?.parse().context("--ticks")?
                }
                file if replay && replay_file.is_none() && !file.starts_with("--") => {
                    replay_file = Some(PathBuf::from(file));
                }
                "edit" if first => options.command = Command::Edit,
                "mcp" if first => options.command = Command::Mcp,
                "--data" => options.data = Some(value("--data")?.into()),
                "--no-vsync" => options.vsync = false,
                "--map" => options.map = Some(value("--map")?),
                "--spectate" => {
                    options.map = Some(value("--spectate")?);
                    options.spectate = true;
                }
                "--players" => {
                    let n: usize = value("--players")?.parse().context("--players")?;
                    if n == 0 {
                        bail!("--players needs at least one player");
                    }
                    options.players = Some(n);
                }
                "--tick" => options.tick = value("--tick")?.parse().context("--tick")?,
                "--gallery" => options.gallery = true,
                "--only" => options.only = Some(value("--only")?),
                "--crowd" => options.crowd = Some(value("--crowd")?.parse().context("--crowd")?),
                "--races" => {
                    let text = value("--races")?;
                    options.races = Some(text.split(',').map(|r| r.trim().to_string()).collect());
                }
                "--colors" => {
                    let text = value("--colors")?;
                    let colors = text
                        .split(',')
                        .map(|c| c.trim().parse::<usize>())
                        .collect::<Result<_, _>>()
                        .context("--colors i,j,…: player colour indices")?;
                    options.colors = Some(colors);
                }
                "--camera" => options.camera = Some(CameraChoice::parse(&value("--camera")?)?),
                "--hour" => options.hour = Some(value("--hour")?.parse().context("--hour")?),
                "--time" => options.time = value("--time")?.parse().context("--time")?,
                "--frames" => options.frames = value("--frames")?.parse().context("--frames")?,
                "--no-shadows" => options.shadows = false,
                "--render-scale" => {
                    let scale: f32 = value("--render-scale")?.parse().context("--render-scale")?;
                    if !(canopy_render::quality::MIN_RENDER_SCALE..=1.0).contains(&scale) {
                        bail!("--render-scale must be within 0.5–1");
                    }
                    options.quality.render_scale = scale;
                }
                "--no-contact" => options.quality.contact_shadows = false,
                "--preset" => {
                    let text = value("--preset")?;
                    options.quality = canopy_render::Preset::parse(&text)
                        .with_context(|| format!("--preset {text}: low, medium, high or ultra"))?
                        .quality();
                }
                "--shadow-map" => {
                    options.quality.shadow_map =
                        value("--shadow-map")?.parse().context("--shadow-map")?
                }
                "--lod-pixels" => {
                    options.quality.lod_pixels =
                        value("--lod-pixels")?.parse().context("--lod-pixels")?
                }
                "--no-bloom" => options.quality.bloom = false,
                "--haze" => {
                    let text = value("--haze")?;
                    options.quality.haze = canopy_render::HazeQuality::parse(&text)
                        .with_context(|| format!("--haze {text}: off, low, medium or high"))?;
                }
                "--pitch" => {
                    let pitch: f32 = value("--pitch")?.parse().context("--pitch")?;
                    if !(10.0..=89.0).contains(&pitch) {
                        bail!("--pitch must be within 10–89 degrees");
                    }
                    options.pitch = Some(pitch);
                }
                "--movers" => options.movers = value("--movers")?.parse().context("--movers")?,
                "--sequence" => {
                    let text = value("--sequence")?;
                    let (frames, step) = text
                        .split_once(',')
                        .context("--sequence <frames>,<step ticks>")?;
                    options.sequence = Some((
                        frames.trim().parse().context("--sequence frames")?,
                        step.trim().parse().context("--sequence step")?,
                    ));
                }
                "--biome" => options.biome = Some(value("--biome")?),
                "--weather" => options.weather = Some(value("--weather")?),
                "--effect" => options
                    .effects
                    .push(EffectSpec::parse(&value("--effect")?)?),
                "--flight" => options
                    .effects
                    .push(EffectSpec::parse_flight(&value("--flight")?)?),
                "--viewer" => {
                    options.viewer = Some(value("--viewer")?.parse().context("--viewer <slot>")?)
                }
                "--select" => options.select = Selection::parse(&value("--select")?)?,
                "--damage" => {
                    options.damage = value("--damage")?
                        .split(',')
                        .map(|p| p.trim().parse::<f32>())
                        .collect::<Result<_, _>>()
                        .context("--damage <percent>[,<percent>…]")?;
                    if options.damage.iter().any(|p| !(1.0..=100.0).contains(p)) {
                        bail!("--damage takes health shares of 1–100 percent");
                    }
                }
                "--kill" => {
                    let text = value("--kill")?;
                    let (which, ticks) = text.split_once(',').unwrap_or((&text, "40"));
                    options.kill = Some((
                        which.trim().to_owned(),
                        ticks
                            .trim()
                            .parse()
                            .context("--kill <text>[,<ticks before>]")?,
                    ));
                }
                "--health" => {
                    use canopy_scene::world_ui::HealthBars;
                    options.health = match value("--health")?.as_str() {
                        "damaged" => HealthBars::Damaged,
                        "always" => HealthBars::Always,
                        "selected" => HealthBars::Selected,
                        other => bail!("--health {other}: damaged, always or selected"),
                    };
                }
                "--play" => options.play = true,
                "--observe" => options.observe = true,
                "--input" => options.input = value("--input")?,
                "--out" => out = Some(PathBuf::from(value("--out")?)),
                "--size" => {
                    let text = value("--size")?;
                    let (w, h) = text.split_once('x').context("--size is <width>x<height>")?;
                    size = [w.parse().context("--size")?, h.parse().context("--size")?];
                }
                other => bail!(
                    "unknown option {other} (expected --data, --no-vsync, --map, --spectate, --players, --races, --tick, --crowd, --gallery, --only, --colors, --camera, --hour, --render-scale, --no-contact, --no-bloom, --haze, --preset, --lod-pixels, --shadow-map, --biome, --weather; shot: --out, --size, --time, --frames, --no-shadows, --pitch, --movers, --sequence, --effect, --flight, --viewer, --select, --health, --damage, --kill, --play, --observe, --input)"
                ),
            }
            first = false;
        }
        if replay {
            let file =
                replay_file.context("replay needs a file: canopy replay <file.utcr> [--verify]")?;
            options.command = Command::Replay {
                file,
                verify,
                record: record.map(|m| (m, record_ticks)),
            };
        } else if capture {
            let out = out.context("capture-screens needs --out <folder>")?;
            if !(0.5..=4.0).contains(&scale) {
                bail!("--scale must be within 0.5–4");
            }
            options.command = Command::CaptureScreens {
                out,
                size,
                scale,
                only,
            };
        } else if shot {
            let out = out.context("shot needs --out <png>")?;
            if options.map.is_none() {
                bail!("shot needs --map <id>");
            }
            if size[0] == 0 || size[1] == 0 || size[0] > 8192 || size[1] > 8192 {
                bail!("--size must be within 1–8192");
            }
            options.command = Command::Shot { out, size };
        } else if out.is_some() {
            bail!("--out belongs to `shot`");
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options> {
        Options::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn replay_takes_a_file_and_verifies_or_records() {
        let o = parse(&["replay", "--data", "/d", "a.utcr", "--verify"]).unwrap();
        assert_eq!(o.data, Some(PathBuf::from("/d")));
        assert_eq!(
            o.command,
            Command::Replay {
                file: "a.utcr".into(),
                verify: true,
                record: None
            }
        );
        let o = parse(&["replay", "out.utcr", "--record", "m", "--ticks", "400"]).unwrap();
        assert_eq!(
            o.command,
            Command::Replay {
                file: "out.utcr".into(),
                verify: false,
                record: Some(("m".into(), 400))
            }
        );
        assert!(parse(&["replay", "--verify"]).is_err(), "a file is needed");
    }

    #[test]
    fn reads_every_option() {
        let options = parse(&[
            "--data",
            "repo",
            "--no-vsync",
            "--map",
            "m",
            "--hour",
            "9.5",
        ])
        .unwrap();
        assert_eq!(options.data, Some(PathBuf::from("repo")));
        assert!(!options.vsync);
        assert_eq!(options.map.as_deref(), Some("m"));
        assert_eq!(options.hour, Some(9.5));
        assert_eq!(options.command, Command::Play);
        let graphics = parse(&["--render-scale", "0.75", "--no-bloom", "--haze", "low"]).unwrap();
        assert_eq!(graphics.quality.render_scale, 0.75);
        assert!(!graphics.quality.bloom && graphics.quality.contact_shadows);
        assert_eq!(graphics.quality.haze, canopy_render::HazeQuality::Low);
        assert!(parse(&["--render-scale", "2"]).is_err());
        assert!(parse(&["--haze", "ultra"]).is_err());
        assert_eq!(parse(&[]).unwrap().data, None);
        assert_eq!(
            parse(&["edit", "--data", "repo"]).unwrap().command,
            Command::Edit
        );
    }

    #[test]
    fn shots_need_a_map_and_an_output() {
        let options = parse(&[
            "shot",
            "--map",
            "m",
            "--camera",
            "117.5,225.5,1",
            "--out",
            "a.png",
            "--size",
            "640x360",
        ])
        .unwrap();
        assert_eq!(
            options.command,
            Command::Shot {
                out: "a.png".into(),
                size: [640, 360]
            }
        );
        assert_eq!(
            options.camera,
            Some(CameraChoice::Pose(CameraSpec {
                x: 117.5,
                z: 225.5,
                zoom: 1.0,
                yaw: 0.0
            }))
        );
        assert_eq!(
            CameraChoice::parse("battle").unwrap(),
            CameraChoice::Battle(1.0)
        );
        assert_eq!(
            CameraChoice::parse("battle,0.6").unwrap(),
            CameraChoice::Battle(0.6)
        );
        assert!(CameraChoice::parse("battlefield").is_err());
        assert_eq!(CameraSpec::parse("1,2,3,45").unwrap().yaw, 45.0);
        assert!(parse(&["shot", "--map", "m"]).is_err());
        assert!(parse(&["shot", "--out", "a.png"]).is_err());
        assert!(parse(&["--out", "a.png"]).is_err());
        assert!(CameraSpec::parse("1,2").is_err());
    }

    #[test]
    fn spectating_names_the_map_and_players() {
        let options = parse(&["--spectate", "m", "--players", "2", "--tick", "400"]).unwrap();
        assert_eq!(options.map.as_deref(), Some("m"));
        assert!(options.spectate);
        assert_eq!((options.players, options.tick), (Some(2), 400));
        assert!(parse(&["--spectate", "m", "--players", "0"]).is_err());
    }

    #[test]
    fn rejects_unknown_and_incomplete_options() {
        assert!(parse(&["--fast"]).is_err());
        assert!(parse(&["--data"]).is_err());
        assert!(parse(&["--map", "m", "shot"]).is_err());
    }
}
