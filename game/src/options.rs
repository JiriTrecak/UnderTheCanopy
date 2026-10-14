//! Command-line options.
//!
//! ```text
//! canopy [--data <folder>] [--no-vsync] [--map <id> | --spectate <id>] [--players <n>]
//!        [--tick <n>] [--camera <x,z,zoom[,yaw]>] [--hour <h>]
//! canopy shot --map <id> --camera <x,z,zoom[,yaw] | battle[,zoom]> --out <png>
//!             [--tick <n>] [--players <n>] [--size <w>x<h>] [--hour <h>] [--time <s>]
//!             [--frames <n>] [--no-shadows] [--data <folder>]
//! ```
//!
//! `--map` shows a match's start, `--spectate` plays it (computer players on every start,
//! or the first `--players`); `--tick` fast-forwards the match first. A shot's `battle`
//! camera looks at the largest fight of that tick.

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

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Open a window.
    Play,
    /// Render offscreen and write a PNG.
    Shot { out: PathBuf, size: [u32; 2] },
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
    /// Run the match (AI players) instead of showing its start.
    pub spectate: bool,
    /// Computer players on the map's first starts (all by default).
    pub players: Option<usize>,
    /// Simulate to this tick before showing anything.
    pub tick: u32,
    pub camera: Option<CameraChoice>,
    /// Hour of the day; the map's own when absent.
    pub hour: Option<f32>,
    /// Animation time of a shot, seconds (wind, canopy drift).
    pub time: f32,
    /// Frames a shot renders before the captured one (for timings).
    pub frames: u32,
    pub shadows: bool,
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
            tick: 0,
            camera: None,
            hour: None,
            time: 12.0,
            frames: 1,
            shadows: true,
        };
        let mut out = None;
        let mut size = [1920, 1080];
        let mut shot = false;
        let mut first = true;
        while let Some(arg) = args.next() {
            let mut value =
                |name: &str| args.next().with_context(|| format!("{name} needs a value"));
            match arg.as_str() {
                "shot" if first => shot = true,
                "play" if first => {}
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
                "--camera" => options.camera = Some(CameraChoice::parse(&value("--camera")?)?),
                "--hour" => options.hour = Some(value("--hour")?.parse().context("--hour")?),
                "--time" => options.time = value("--time")?.parse().context("--time")?,
                "--frames" => options.frames = value("--frames")?.parse().context("--frames")?,
                "--no-shadows" => options.shadows = false,
                "--out" => out = Some(PathBuf::from(value("--out")?)),
                "--size" => {
                    let text = value("--size")?;
                    let (w, h) = text.split_once('x').context("--size is <width>x<height>")?;
                    size = [w.parse().context("--size")?, h.parse().context("--size")?];
                }
                other => bail!(
                    "unknown option {other} (expected --data, --no-vsync, --map, --spectate, --players, --tick, --camera, --hour; shot: --out, --size, --time, --frames, --no-shadows)"
                ),
            }
            first = false;
        }
        if shot {
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
        assert_eq!(parse(&[]).unwrap().data, None);
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
