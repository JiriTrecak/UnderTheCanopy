//! Command-line options.

use anyhow::{Result, bail};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// Game data folder; overrides the bundled location (`just play` passes the repository).
    pub data: Option<PathBuf>,
    /// Present without waiting for vertical sync, for measuring frame cost.
    pub vsync: bool,
}

impl Options {
    pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut options = Self {
            data: None,
            vsync: true,
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--data" => match args.next() {
                    Some(dir) => options.data = Some(dir.into()),
                    None => bail!("--data needs a folder"),
                },
                "--no-vsync" => options.vsync = false,
                other => bail!("unknown option {other} (expected --data <folder> or --no-vsync)"),
            }
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
        let options = parse(&["--data", "repo", "--no-vsync"]).unwrap();
        assert_eq!(options.data, Some(PathBuf::from("repo")));
        assert!(!options.vsync);
        assert_eq!(parse(&[]).unwrap().data, None);
    }

    #[test]
    fn rejects_unknown_and_incomplete_options() {
        assert!(parse(&["--fast"]).is_err());
        assert!(parse(&["--data"]).is_err());
    }
}
