//! Command-line options.
//!
//! ```text
//! canopy [--data <folder>] [--no-vsync] [--map <id>] [--camera <x,z,zoom[,yaw]>] [--hour <h>]
//!        [graphics]
//! canopy shot --map <id> --camera <x,z,zoom[,yaw]> --out <png> [--size <w>x<h>] [--hour <h>]
//!             [--time <s>] [--frames <n>] [--no-shadows] [--pitch <deg>] [--movers <n>]
//!             [--data <folder>] [graphics]
//! graphics: [--render-scale <0.5–1>] [--no-contact] [--no-bloom] [--haze off|low|medium|high]
//!           [--biome <id>] (preview the map in another biome's look)
//! ```

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
    pub camera: Option<CameraSpec>,
    /// Hour of the day; the map's own when absent.
    pub hour: Option<f32>,
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
    /// Another biome's look for the map (previews).
    pub biome: Option<String>,
}

impl Options {
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self {
            command: Command::Play,
            data: None,
            vsync: true,
            map: None,
            camera: None,
            hour: None,
            time: 12.0,
            frames: 1,
            shadows: true,
            quality: canopy_render::Quality::default(),
            pitch: None,
            movers: 0,
            biome: None,
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
                "--camera" => options.camera = Some(CameraSpec::parse(&value("--camera")?)?),
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
                "--biome" => options.biome = Some(value("--biome")?),
                "--out" => out = Some(PathBuf::from(value("--out")?)),
                "--size" => {
                    let text = value("--size")?;
                    let (w, h) = text.split_once('x').context("--size is <width>x<height>")?;
                    size = [w.parse().context("--size")?, h.parse().context("--size")?];
                }
                other => bail!(
                    "unknown option {other} (expected --data, --no-vsync, --map, --camera, --hour, --render-scale, --no-contact, --no-bloom, --haze, --biome; shot: --out, --size, --time, --frames, --no-shadows, --pitch, --movers)"
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
        let graphics = parse(&["--render-scale", "0.75", "--no-bloom", "--haze", "low"]).unwrap();
        assert_eq!(graphics.quality.render_scale, 0.75);
        assert!(!graphics.quality.bloom && graphics.quality.contact_shadows);
        assert_eq!(graphics.quality.haze, canopy_render::HazeQuality::Low);
        assert!(parse(&["--render-scale", "2"]).is_err());
        assert!(parse(&["--haze", "ultra"]).is_err());
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
            Some(CameraSpec {
                x: 117.5,
                z: 225.5,
                zoom: 1.0,
                yaw: 0.0
            })
        );
        assert_eq!(CameraSpec::parse("1,2,3,45").unwrap().yaw, 45.0);
        assert!(parse(&["shot", "--map", "m"]).is_err());
        assert!(parse(&["shot", "--out", "a.png"]).is_err());
        assert!(parse(&["--out", "a.png"]).is_err());
        assert!(CameraSpec::parse("1,2").is_err());
    }

    #[test]
    fn rejects_unknown_and_incomplete_options() {
        assert!(parse(&["--fast"]).is_err());
        assert!(parse(&["--data"]).is_err());
        assert!(parse(&["--map", "m", "shot"]).is_err());
    }
}
