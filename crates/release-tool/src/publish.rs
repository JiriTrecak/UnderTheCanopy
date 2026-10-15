//! `release-publish`: uploads a staged artifact to the release host (S3 behind CloudFront)
//! and advances the stable feed, as the old publisher did.
//!
//! Without `--apply` it only verifies: the package's hash, size, signature and version, the
//! notes against the log, and the AWS account. With `--apply` it takes the publisher lock,
//! refuses downgrades and stranding other platforms, uploads every versioned file
//! immutably (never overwriting), verifies the stored and the public copies byte for byte,
//! updates the changelog index and writes `stable.json` last: the feed is the commit
//! point, so a failure anywhere before it leaves the published release unchanged.

use crate::{media, package::digest_file, stage::json};
use anyhow::{Context, Result, anyhow, bail};
use canopy_release::{
    Artifact, ChangelogIndex, ChangelogLink, PlatformPackage, ReleaseLog, StableFeed,
    verify_package,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const MIB: u64 = 1024 * 1024;
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const SHORT: &str = "public, max-age=60";
const LOCK_KEY: &str = "publishing.lock";

/// `releases/deployment.json`: the release host's identifiers.
#[derive(Debug, Clone, Deserialize)]
pub struct Deployment {
    pub account: String,
    pub region: String,
    #[serde(rename = "BucketName")]
    pub bucket: String,
    #[serde(rename = "BaseUrl")]
    pub base_url: String,
}

/// A failed `aws` invocation, with what it printed on stderr.
#[derive(Debug)]
pub struct AwsError {
    pub stderr: String,
}

impl std::fmt::Display for AwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "aws: {}", self.stderr.trim())
    }
}

impl std::error::Error for AwsError {}

/// Runs the AWS CLI (the publisher keeps credentials in the CLI's providers).
pub trait Aws {
    fn run(&self, args: &[String]) -> Result<String, AwsError>;
}

/// The installed `aws` command, in the deployment's region.
pub struct AwsCli {
    pub region: String,
    pub profile: Option<String>,
}

impl Aws for AwsCli {
    fn run(&self, args: &[String]) -> Result<String, AwsError> {
        let mut command = Command::new("aws");
        command
            .args(args)
            .args([
                "--region",
                &self.region,
                "--no-cli-pager",
                "--output",
                "json",
            ])
            .env("AWS_PAGER", "")
            .stdin(Stdio::null());
        if let Some(profile) = &self.profile {
            command.env("AWS_PROFILE", profile);
        }
        let output = command.output().map_err(|e| AwsError {
            stderr: format!("cannot run the aws command: {e}"),
        })?;
        if !output.status.success() {
            return Err(AwsError {
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// A ranged response from the public CDN.
pub struct RangeResponse {
    pub status: u16,
    pub content_range: Option<String>,
    pub body: Vec<u8>,
}

/// The public side of the release host.
pub trait Cdn {
    /// The whole file; an error unless the response is a success.
    fn get(&self, url: &str) -> Result<Vec<u8>>;
    fn get_range(&self, url: &str, start: u64, end: u64) -> Result<RangeResponse>;
}

/// HTTPS with the operating system's certificate verifier.
pub struct HttpCdn {
    client: reqwest::blocking::Client,
}

impl HttpCdn {
    pub fn new() -> Result<Self> {
        use rustls_platform_verifier::BuilderVerifierExt;
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_platform_verifier()?
            .with_no_client_auth();
        let client = reqwest::blocking::Client::builder()
            .use_preconfigured_tls(tls)
            .build()?;
        Ok(HttpCdn { client })
    }
}

impl Cdn for HttpCdn {
    fn get(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()?;
        if !response.status().is_success() {
            bail!("{url}: HTTP {}", response.status());
        }
        Ok(response.bytes()?.to_vec())
    }

    fn get_range(&self, url: &str, start: u64, end: u64) -> Result<RangeResponse> {
        let response = self
            .client
            .get(url)
            .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
            .timeout(Duration::from_secs(120))
            .send()?;
        let status = response.status().as_u16();
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok(RangeResponse {
            status,
            content_range,
            body: response.bytes()?.to_vec(),
        })
    }
}

/// A staged artifact that verified.
pub struct Staged {
    pub artifact: Artifact,
    /// `artifact.json` itself (published as `<platform>.json`).
    pub artifact_path: PathBuf,
    pub package: PathBuf,
    pub media: PathBuf,
}

/// Verifies a staged artifact: its package's hash, size and signature (bound to the
/// version) against `public_key`, its notes against the log, and its gallery.
pub fn verify_staged(artifact_path: &Path, log: &ReleaseLog, public_key: &str) -> Result<Staged> {
    let text = fs::read_to_string(artifact_path)
        .with_context(|| format!("reading {}", artifact_path.display()))?;
    let artifact: Artifact = serde_json::from_str(&text).context("artifact.json")?;
    artifact
        .validate()
        .map_err(|e| anyhow!("artifact.json: {e}"))?;
    let dir = artifact_path.parent().unwrap_or(Path::new("."));
    let package = dir.join(&artifact.filename);
    let (sha256, bytes) = digest_file(&package)?;
    if sha256 != artifact.sha256 || bytes != artifact.bytes {
        bail!("the package differs from the signed build");
    }
    verify_package(
        public_key,
        &artifact.signature,
        &artifact.version.to_string(),
        fs::File::open(&package)?,
        |_| {},
    )
    .context("the package signature")?;
    if log.release(artifact.version) != Some(&artifact.changelog) {
        bail!("the release notes changed after this build; rebuild before publishing");
    }
    let media = dir.join("media");
    for image in artifact.changelog.changes.images.iter().flatten() {
        media::verify_image(&media, image, false)?;
    }
    Ok(Staged {
        artifact,
        artifact_path: artifact_path.to_owned(),
        package,
        media,
    })
}

/// Refuses any AWS account but the release account (other local accounts belong to
/// unrelated projects).
pub fn check_identity(aws: &dyn Aws, deployment: &Deployment) -> Result<()> {
    let out = aws.run(&args(&["sts", "get-caller-identity"]))?;
    let identity: serde_json::Value = serde_json::from_str(&out)?;
    let account = identity["Account"].as_str().unwrap_or("");
    if account != deployment.account {
        bail!(
            "wrong AWS account ({account}); select the release account {} before publishing (e.g. --profile canopy)",
            deployment.account
        );
    }
    Ok(())
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).to_owned()).collect()
}

fn is_precondition_failed(e: &anyhow::Error) -> bool {
    e.downcast_ref::<AwsError>()
        .is_some_and(|e| e.stderr.contains("PreconditionFailed"))
}

/// Uploads a verified artifact and advances the feed (`--apply`).
pub struct Publisher<'a> {
    pub aws: &'a dyn Aws,
    pub cdn: &'a dyn Cdn,
    pub bucket: String,
    pub base_url: String,
    /// Packages above this upload in parts.
    pub multipart_above: u64,
    pub part_size: u64,
    /// Ranged verification reads this much at a time.
    pub verify_chunk: u64,
}

impl<'a> Publisher<'a> {
    pub fn new(aws: &'a dyn Aws, cdn: &'a dyn Cdn, deployment: &Deployment) -> Self {
        Publisher {
            aws,
            cdn,
            bucket: deployment.bucket.clone(),
            base_url: deployment.base_url.clone(),
            multipart_above: 64 * MIB,
            part_size: 32 * MIB,
            verify_chunk: 32 * MIB,
        }
    }

    fn aws(&self, a: &[&str]) -> Result<String> {
        Ok(self.aws.run(&args(a))?)
    }

    pub fn publish(&self, staged: &Staged) -> Result<()> {
        let id = lock_id();
        let temp = std::env::temp_dir().join(format!("canopy-publish-{id}"));
        fs::create_dir_all(&temp)?;
        let mut locked = false;
        let result = self.publish_locked(staged, &temp, &id, &mut locked);
        let mut cleanup = Ok(());
        if locked
            && let Err(e) = self.aws(&[
                "s3api",
                "delete-object",
                "--bucket",
                &self.bucket,
                "--key",
                LOCK_KEY,
            ])
        {
            eprintln!("warning: could not remove {LOCK_KEY}: {e:#}");
            cleanup = Err(e);
        }
        let _ = fs::remove_dir_all(&temp);
        result.and(cleanup)
    }

    fn put(
        &self,
        key: &str,
        body: &Path,
        kind: &str,
        immutable: bool,
        conditional: bool,
    ) -> Result<String> {
        let body = body.to_string_lossy();
        let mut a = vec![
            "s3api",
            "put-object",
            "--bucket",
            &self.bucket,
            "--key",
            key,
            "--body",
            &body,
            "--content-type",
            kind,
            "--cache-control",
            if immutable { IMMUTABLE } else { SHORT },
        ];
        if conditional {
            a.extend(["--if-none-match", "*"]);
        }
        self.aws(&a)
    }

    /// A JSON object from the bucket, or `None` if there is none.
    fn read<T: serde::de::DeserializeOwned>(&self, key: &str, temp: &Path) -> Result<Option<T>> {
        let dest = temp.join(format!("read-{}", key.replace('/', "_")));
        match self.aws(&[
            "s3api",
            "get-object",
            "--bucket",
            &self.bucket,
            "--key",
            key,
            &dest.to_string_lossy(),
        ]) {
            Ok(_) => {
                let text = fs::read_to_string(&dest)?;
                Ok(Some(
                    serde_json::from_str(&text).with_context(|| format!("{key} does not parse"))?,
                ))
            }
            Err(e)
                if e.downcast_ref::<AwsError>()
                    .is_some_and(|e| e.stderr.contains("NoSuchKey")) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    fn json_file(&self, temp: &Path, name: &str, value: &impl serde::Serialize) -> Result<PathBuf> {
        let path = temp.join(name);
        fs::write(&path, json(value))?;
        Ok(path)
    }

    fn publish_locked(
        &self,
        staged: &Staged,
        temp: &Path,
        id: &str,
        locked: &mut bool,
    ) -> Result<()> {
        let artifact = &staged.artifact;
        let version = artifact.version.to_string();
        let lock = self.json_file(
            temp,
            "lock.json",
            &serde_json::json!({"id": id, "version": version, "startedAt": crate::repo::iso_now()}),
        )?;
        self.put(LOCK_KEY, &lock, "application/json", false, true).map_err(|e| {
            if is_precondition_failed(&e) {
                anyhow!(
                    "another publish holds {LOCK_KEY}; if it has certainly stopped, inspect and remove only that lock"
                )
            } else {
                e
            }
        })?;
        *locked = true;

        let previous: Option<StableFeed> = self.read("updates/stable.json", temp)?;
        if let Some(previous) = &previous {
            if artifact.version < previous.version {
                bail!(
                    "refusing to downgrade the stable feed ({} → {version})",
                    previous.version
                );
            }
            if previous.version != artifact.version
                && previous.platforms.keys().any(|p| *p != artifact.platform)
            {
                bail!(
                    "stable serves other platforms; prepare a combined multi-platform promotion before advancing the feed. This single-platform publisher must not strand existing users."
                );
            }
        }
        let prefix = format!("updates/releases/{version}");
        let public = format!("{}/releases/{version}", self.base_url);
        let platform = PlatformPackage {
            url: format!("{public}/{}", artifact.filename),
            signature: artifact.signature.clone(),
        };
        if let Some(previous) = &previous
            && previous.version == artifact.version
            && previous
                .platforms
                .get(&artifact.platform)
                .is_some_and(|p| *p != platform)
        {
            bail!("this platform and version is already published with another package");
        }

        println!("Uploading and verifying the stored package…");
        self.immutable(
            &format!("{prefix}/{}", artifact.filename),
            &staged.package,
            "application/octet-stream",
            temp,
        )?;
        let signature = temp.join("signature.sig");
        fs::write(&signature, format!("{}\n", artifact.signature))?;
        self.immutable(
            &format!("{prefix}/{}.sig", artifact.filename),
            &signature,
            "text/plain",
            temp,
        )?;
        for image in artifact.changelog.changes.images.iter().flatten() {
            let file = match media::verify_image(&staged.media, image, false)? {
                media::Checked::Verified(file) => file,
                media::Checked::LfsPointer(_) => unreachable!("pointers are refused"),
            };
            self.immutable(
                &format!("{prefix}/media/{}", image.file),
                &file,
                "image/webp",
                temp,
            )?;
            let bytes = self
                .cdn
                .get(&format!("{public}/media/{}", image.file))
                .with_context(|| {
                    format!("the public gallery image {} is unavailable", image.file)
                })?;
            if format!("{}.webp", crate::package::hex(&Sha256::digest(&bytes))) != image.file {
                bail!("the public gallery image differs: {}", image.file);
            }
        }
        let changelog = self.json_file(temp, "changelog.json", &artifact.changelog)?;
        self.immutable(
            &format!("{prefix}/changelog.json"),
            &changelog,
            "application/json",
            temp,
        )?;
        self.immutable(
            &format!("{prefix}/{}.json", artifact.platform),
            &staged.artifact_path,
            "application/json",
            temp,
        )?;

        // The exact package must come from the public CDN before the feed advertises it.
        println!("Verifying the public CDN download…");
        let size = artifact.bytes;
        let public_hash = ranged_digest(
            size,
            self.verify_chunk,
            |start, end| {
                let r = self.cdn.get_range(&platform.url, start, end)?;
                if r.status != 206
                    || r.content_range.as_deref() != Some(&format!("bytes {start}-{end}/{size}"))
                {
                    bail!("invalid public package range: HTTP {}", r.status);
                }
                Ok(r.body)
            },
            |p| println!("Public download verified {p}%…"),
        )?;
        if public_hash != artifact.sha256 {
            bail!("public package hash mismatch");
        }

        let link = ChangelogLink {
            version: artifact.version,
            title: artifact.changelog.title.clone(),
            published_at: artifact.changelog.published_at.clone(),
            url: format!("releases/{version}/changelog.json"),
        };
        let mut index: ChangelogIndex =
            self.read("updates/changelog.json", temp)?
                .unwrap_or(ChangelogIndex {
                    schema_version: 1,
                    releases: Vec::new(),
                });
        index.releases.retain(|r| r.version != link.version);
        index.releases.push(link);
        index.releases.sort_by_key(|r| std::cmp::Reverse(r.version));
        let index = self.json_file(temp, "index.json", &index)?;
        self.put(
            "updates/changelog.json",
            &index,
            "application/json",
            false,
            false,
        )?;

        let mut platforms = match previous {
            Some(p) if p.version == artifact.version => p.platforms,
            _ => Default::default(),
        };
        platforms.insert(artifact.platform.clone(), platform);
        let feed = StableFeed {
            version: artifact.version,
            notes: artifact.changelog.plain_notes(),
            pub_date: artifact.changelog.published_at.clone(),
            platforms,
        };
        // The feed is the commit point; previous releases stay served if anything above failed.
        let feed = self.json_file(temp, "stable.json", &feed)?;
        self.put(
            "updates/stable.json",
            &feed,
            "application/json",
            false,
            false,
        )?;
        println!("Published {version}: {}/stable.json", self.base_url);
        Ok(())
    }

    /// Uploads `file` to `key` unless it is already there, never overwriting, then verifies
    /// the stored object byte for byte. Anything else at `key` is a conflict.
    fn immutable(&self, key: &str, file: &Path, kind: &str, temp: &Path) -> Result<()> {
        let (expected, bytes) = digest_file(file)?;
        let conflict =
            || anyhow!("immutable release file conflict: {key}; use a new release version");
        let exists = match self.aws(&[
            "s3api",
            "head-object",
            "--bucket",
            &self.bucket,
            "--key",
            key,
        ]) {
            Ok(out) => {
                let head: serde_json::Value = serde_json::from_str(&out)?;
                if head["ContentLength"].as_u64() != Some(bytes) {
                    return Err(conflict());
                }
                true
            }
            Err(e)
                if e.downcast_ref::<AwsError>().is_some_and(|e| {
                    e.stderr.contains("(404)")
                        || e.stderr.contains("Not Found")
                        || e.stderr.contains("NoSuchKey")
                }) =>
            {
                false
            }
            Err(e) => return Err(e),
        };
        if !exists {
            let uploaded = if bytes > self.multipart_above {
                self.multipart(key, file, kind, temp)
            } else {
                self.put(key, file, kind, true, true).map(|_| ())
            };
            match uploaded {
                // Someone else stored it between the check and the upload: verify theirs.
                Err(e) if is_precondition_failed(&e) => {}
                other => other?,
            }
        }
        let copy = temp.join("verify-range.bin");
        let large = bytes > self.multipart_above;
        let stored = ranged_digest(
            bytes,
            self.verify_chunk,
            |start, end| {
                self.aws(&[
                    "s3api",
                    "get-object",
                    "--bucket",
                    &self.bucket,
                    "--key",
                    key,
                    "--range",
                    &format!("bytes={start}-{end}"),
                    &copy.to_string_lossy(),
                ])?;
                Ok(fs::read(&copy)?)
            },
            |p| {
                if large {
                    println!("Stored package verified {p}%…");
                }
            },
        )?;
        let _ = fs::remove_file(&copy);
        if stored != expected {
            return Err(conflict());
        }
        Ok(())
    }

    /// Bounded-memory upload in parts; completes only if the key is still absent, and
    /// aborts the upload on any failure.
    fn multipart(&self, key: &str, file: &Path, kind: &str, temp: &Path) -> Result<()> {
        let out = self.aws(&[
            "s3api",
            "create-multipart-upload",
            "--bucket",
            &self.bucket,
            "--key",
            key,
            "--content-type",
            kind,
            "--cache-control",
            IMMUTABLE,
        ])?;
        let created: serde_json::Value = serde_json::from_str(&out)?;
        let upload_id = created["UploadId"]
            .as_str()
            .ok_or_else(|| anyhow!("S3 did not return an upload id"))?
            .to_owned();
        let part_file = temp.join("upload-part.bin");
        let result = (|| -> Result<()> {
            let mut source = fs::File::open(file)?;
            let size = source.metadata()?.len();
            let mut buffer = vec![0u8; self.part_size as usize];
            let mut parts = Vec::new();
            let mut offset = 0u64;
            while offset < size {
                let length = self.part_size.min(size - offset) as usize;
                source.seek(SeekFrom::Start(offset))?;
                source
                    .read_exact(&mut buffer[..length])
                    .context("the release package changed during upload")?;
                fs::write(&part_file, &buffer[..length])?;
                let number = (parts.len() + 1).to_string();
                let out = self.aws(&[
                    "s3api",
                    "upload-part",
                    "--bucket",
                    &self.bucket,
                    "--key",
                    key,
                    "--upload-id",
                    &upload_id,
                    "--part-number",
                    &number,
                    "--body",
                    &part_file.to_string_lossy(),
                ])?;
                let part: serde_json::Value = serde_json::from_str(&out)?;
                let etag = part["ETag"]
                    .as_str()
                    .ok_or_else(|| anyhow!("S3 did not confirm an uploaded part"))?;
                parts.push(serde_json::json!({"PartNumber": parts.len() + 1, "ETag": etag}));
                offset += length as u64;
                println!("Uploaded {}% ({} parts).", offset * 100 / size, parts.len());
            }
            let manifest = temp.join("upload-parts.json");
            fs::write(&manifest, serde_json::json!({"Parts": parts}).to_string())?;
            self.aws(&[
                "s3api",
                "complete-multipart-upload",
                "--bucket",
                &self.bucket,
                "--key",
                key,
                "--upload-id",
                &upload_id,
                "--multipart-upload",
                &format!("file://{}", manifest.display()),
                "--if-none-match",
                "*",
            ])?;
            Ok(())
        })();
        let _ = fs::remove_file(&part_file);
        if result.is_err()
            && self
                .aws(&[
                    "s3api",
                    "abort-multipart-upload",
                    "--bucket",
                    &self.bucket,
                    "--key",
                    key,
                    "--upload-id",
                    &upload_id,
                ])
                .is_err()
        {
            println!(
                "Multipart cleanup could not finish; the bucket lifecycle removes abandoned parts."
            );
        }
        result
    }
}

/// SHA-256 of `size` bytes read in ranges of `chunk` (inclusive `start..=end`), each tried
/// up to three times and required to be complete; `progress` sees each new tenth.
pub fn ranged_digest(
    size: u64,
    chunk: u64,
    mut read: impl FnMut(u64, u64) -> Result<Vec<u8>>,
    mut progress: impl FnMut(u64),
) -> Result<String> {
    let mut hash = Sha256::new();
    let mut last = 0;
    let mut start = 0;
    while start < size {
        let end = size.min(start + chunk) - 1;
        let mut attempt = 0;
        let bytes = loop {
            let result = read(start, end).and_then(|b| {
                if b.len() as u64 != end - start + 1 {
                    bail!("incomplete verification range");
                }
                Ok(b)
            });
            match result {
                Ok(b) => break b,
                Err(e) if attempt == 2 => return Err(e),
                Err(_) => attempt += 1,
            }
        };
        hash.update(&bytes);
        let percent = (end + 1) * 10 / size * 10;
        if percent > last {
            progress(percent);
            last = percent;
        }
        start = end + 1;
    }
    Ok(crate::package::hex(&hash.finalize()))
}

/// An id for the publisher lock (who holds it).
fn lock_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let digest = Sha256::digest(format!("{nanos}:{}", std::process::id()));
    crate::package::hex(&digest)[..32].to_owned()
}

#[cfg(test)]
mod tests;
