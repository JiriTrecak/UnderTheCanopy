//! The publisher against an in-memory bucket behind a fake `aws` and CDN.

use super::*;
use base64::Engine;
use canopy_release::{ChangeSet, ReleaseEntry, Version};
use std::{cell::RefCell, collections::BTreeMap};

/// S3 as the AWS CLI shows it: objects, conditional writes, ranges, multipart uploads and
/// the error texts the publisher reacts to.
#[derive(Default)]
struct FakeAws {
    account: String,
    objects: RefCell<BTreeMap<String, Vec<u8>>>,
    parts: RefCell<BTreeMap<u32, Vec<u8>>>,
    /// Every call as `operation key`.
    calls: RefCell<Vec<String>>,
    /// Fails the first call whose `operation key` starts with this.
    fail: RefCell<Option<String>>,
}

impl FakeAws {
    fn new() -> Self {
        FakeAws {
            account: "123".into(),
            ..Default::default()
        }
    }

    fn object(&self, key: &str) -> Option<Vec<u8>> {
        self.objects.borrow().get(key).cloned()
    }

    fn json(&self, key: &str) -> serde_json::Value {
        serde_json::from_slice(&self.object(key).unwrap()).unwrap()
    }

    fn calls_of(&self, op: &str) -> usize {
        self.calls
            .borrow()
            .iter()
            .filter(|c| c.starts_with(op))
            .count()
    }
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .map(|i| args[i + 1].as_str())
}

fn err(s: &str) -> Result<String, AwsError> {
    Err(AwsError { stderr: s.into() })
}

impl Aws for FakeAws {
    fn run(&self, args: &[String]) -> Result<String, AwsError> {
        let op = args[..2.min(args.len())].join(" ");
        let key = opt(args, "--key").unwrap_or("").to_owned();
        let call = format!("{} {key}", args[1]);
        self.calls.borrow_mut().push(call.clone());
        if self
            .fail
            .borrow()
            .as_deref()
            .is_some_and(|f| call.starts_with(f))
        {
            self.fail.borrow_mut().take();
            return err("An error occurred (InternalError): injected");
        }
        let mut objects = self.objects.borrow_mut();
        let conditional = opt(args, "--if-none-match") == Some("*");
        match op.as_str() {
            "sts get-caller-identity" => Ok(format!(r#"{{"Account":"{}"}}"#, self.account)),
            "s3api put-object" => {
                if conditional && objects.contains_key(&key) {
                    return err(
                        "An error occurred (PreconditionFailed) when calling the PutObject operation",
                    );
                }
                let body = fs::read(opt(args, "--body").unwrap()).unwrap();
                objects.insert(key, body);
                Ok(r#"{"ETag":"\"x\""}"#.into())
            }
            "s3api head-object" => match objects.get(&key) {
                Some(o) => Ok(format!(r#"{{"ContentLength":{}}}"#, o.len())),
                None => {
                    err("An error occurred (404) when calling the HeadObject operation: Not Found")
                }
            },
            "s3api get-object" => {
                let Some(o) = objects.get(&key) else {
                    return err(
                        "An error occurred (NoSuchKey) when calling the GetObject operation",
                    );
                };
                let bytes = match opt(args, "--range") {
                    Some(r) => {
                        let (a, b) = r.strip_prefix("bytes=").unwrap().split_once('-').unwrap();
                        let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
                        o[a..=b.min(o.len() - 1)].to_vec()
                    }
                    None => o.clone(),
                };
                fs::write(args.last().unwrap(), bytes).unwrap();
                Ok("{}".into())
            }
            "s3api delete-object" => {
                objects.remove(&key);
                Ok("{}".into())
            }
            "s3api create-multipart-upload" => {
                self.parts.borrow_mut().clear();
                Ok(r#"{"UploadId":"upload-1"}"#.into())
            }
            "s3api upload-part" => {
                let n: u32 = opt(args, "--part-number").unwrap().parse().unwrap();
                let body = fs::read(opt(args, "--body").unwrap()).unwrap();
                self.parts.borrow_mut().insert(n, body);
                Ok(format!(r#"{{"ETag":"\"etag-{n}\""}}"#))
            }
            "s3api complete-multipart-upload" => {
                if conditional && objects.contains_key(&key) {
                    return err(
                        "An error occurred (PreconditionFailed) when calling the CompleteMultipartUpload operation",
                    );
                }
                let manifest = opt(args, "--multipart-upload").unwrap();
                let manifest: serde_json::Value = serde_json::from_str(
                    &fs::read_to_string(manifest.strip_prefix("file://").unwrap()).unwrap(),
                )
                .unwrap();
                let parts = self.parts.borrow();
                let mut body = Vec::new();
                for p in manifest["Parts"].as_array().unwrap() {
                    let n = p["PartNumber"].as_u64().unwrap() as u32;
                    assert_eq!(p["ETag"], format!("\"etag-{n}\""));
                    body.extend_from_slice(&parts[&n]);
                }
                objects.insert(key, body);
                Ok("{}".into())
            }
            "s3api abort-multipart-upload" => {
                self.parts.borrow_mut().clear();
                Ok("{}".into())
            }
            other => panic!("unexpected aws {other}"),
        }
    }
}

/// CloudFront: serves the bucket's `updates/` prefix at the base URL.
struct FakeCdn<'a>(&'a FakeAws);

impl FakeCdn<'_> {
    fn key(url: &str) -> String {
        format!("updates/{}", url.strip_prefix("https://cdn.test/").unwrap())
    }
}

impl Cdn for FakeCdn<'_> {
    fn get(&self, url: &str) -> Result<Vec<u8>> {
        self.0.object(&Self::key(url)).ok_or_else(|| anyhow!("404"))
    }

    fn get_range(&self, url: &str, start: u64, end: u64) -> Result<RangeResponse> {
        let o = self
            .0
            .object(&Self::key(url))
            .ok_or_else(|| anyhow!("404"))?;
        Ok(RangeResponse {
            status: 206,
            content_range: Some(format!("bytes {start}-{end}/{}", o.len())),
            body: o[start as usize..=end as usize].to_vec(),
        })
    }
}

struct Fixture {
    dir: PathBuf,
    public_key: String,
    keys: minisign::KeyPair,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("canopy-publish-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let keys = minisign::KeyPair::generate_encrypted_keypair(Some(String::new())).unwrap();
        let public_key =
            base64::engine::general_purpose::STANDARD.encode(keys.pk.to_box().unwrap().to_string());
        Fixture {
            dir,
            public_key,
            keys,
        }
    }

    fn entry(version: &str) -> ReleaseEntry {
        ReleaseEntry {
            changes: ChangeSet {
                added: vec!["A thing.".into()],
                ..Default::default()
            },
            version: version.parse().unwrap(),
            published_at: "2026-10-15T10:00:00.000Z".into(),
            title: format!("Release {version}"),
        }
    }

    /// Stages a signed package of `size` bytes for `version` and returns it with its log.
    fn stage(&self, version: &str, platform: &str, size: usize) -> (Staged, ReleaseLog) {
        let out = self.dir.join(version).join(platform);
        fs::create_dir_all(&out).unwrap();
        let filename = format!("under-the-canopy-{version}-{platform}.app.tar.gz");
        let bytes: Vec<u8> = (0..size).map(|i| (i * 7 + version.len()) as u8).collect();
        fs::write(out.join(&filename), &bytes).unwrap();
        let sk = crate::package::secret_key_from_tauri(
            &base64::engine::general_purpose::STANDARD
                .encode(self.keys.sk.to_box(None).unwrap().to_string()),
            String::new(),
        )
        .unwrap();
        let signature = crate::package::sign_package(
            &sk,
            &out.join(&filename),
            "Under the Canopy.app.tar.gz",
            version,
            1,
        )
        .unwrap();
        let artifact = Artifact {
            schema_version: 1,
            version: version.parse().unwrap(),
            platform: platform.into(),
            filename,
            sha256: crate::package::hex(&Sha256::digest(&bytes)),
            bytes: bytes.len() as u64,
            signature,
            changelog: Self::entry(version),
        };
        let path = out.join("artifact.json");
        fs::write(&path, json(&artifact)).unwrap();
        let log = ReleaseLog {
            schema_version: 1,
            unreleased: ChangeSet::default(),
            releases: vec![Self::entry(version)],
        };
        (verify_staged(&path, &log, &self.public_key).unwrap(), log)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn deployment() -> Deployment {
    Deployment {
        account: "123".into(),
        region: "eu-west-2".into(),
        bucket: "bucket".into(),
        base_url: "https://cdn.test".into(),
    }
}

fn publisher<'a>(aws: &'a FakeAws, cdn: &'a FakeCdn<'a>) -> Publisher<'a> {
    Publisher {
        multipart_above: 1000,
        part_size: 300,
        verify_chunk: 256,
        ..Publisher::new(aws, cdn, &deployment())
    }
}

#[test]
fn a_release_is_uploaded_verified_and_the_feed_written_last() {
    let fixture = Fixture::new("full");
    let (staged, _) = fixture.stage("0.3.0", "darwin-aarch64", 2500);
    let aws = FakeAws::new();
    let cdn = FakeCdn(&aws);
    check_identity(&aws, &deployment()).unwrap();
    publisher(&aws, &cdn).publish(&staged).unwrap();

    let prefix = "updates/releases/0.3.0";
    let package = fs::read(&staged.package).unwrap();
    assert_eq!(
        aws.object(&format!("{prefix}/{}", staged.artifact.filename))
            .unwrap(),
        package
    );
    // Large packages go up in parts and complete only if the key is absent.
    assert_eq!(aws.calls_of("upload-part"), 9);
    assert_eq!(
        aws.object(&format!("{prefix}/{}.sig", staged.artifact.filename))
            .unwrap(),
        format!("{}\n", staged.artifact.signature).into_bytes()
    );
    assert_eq!(
        aws.object(&format!("{prefix}/darwin-aarch64.json"))
            .unwrap(),
        fs::read(&staged.artifact_path).unwrap()
    );
    assert_eq!(
        aws.json(&format!("{prefix}/changelog.json"))["title"],
        "Release 0.3.0"
    );
    let index = aws.json("updates/changelog.json");
    assert_eq!(index["releases"][0]["url"], "releases/0.3.0/changelog.json");
    let feed: StableFeed =
        serde_json::from_slice(&aws.object("updates/stable.json").unwrap()).unwrap();
    assert_eq!(feed.version, "0.3.0".parse::<Version>().unwrap());
    assert_eq!(feed.notes, "Release 0.3.0\n\nAdded\n- A thing.");
    assert_eq!(
        feed.platforms["darwin-aarch64"].url,
        format!(
            "https://cdn.test/releases/0.3.0/{}",
            staged.artifact.filename
        )
    );
    // The feed is the last write, and the lock is gone.
    let calls = aws.calls.borrow();
    let last_put = calls
        .iter()
        .rev()
        .find(|c| c.starts_with("put-object"))
        .unwrap();
    assert_eq!(last_put, "put-object updates/stable.json");
    assert_eq!(calls.last().unwrap(), "delete-object publishing.lock");
    assert!(aws.object(LOCK_KEY).is_none());
    drop(calls);

    // Publishing the same artifact again is a safe retry; the next patch advances the feed.
    publisher(&aws, &cdn).publish(&staged).unwrap();
    let (next, _) = fixture.stage("0.3.1", "darwin-aarch64", 500);
    publisher(&aws, &cdn).publish(&next).unwrap();
    let index = aws.json("updates/changelog.json");
    assert_eq!(index["releases"].as_array().unwrap().len(), 2);
    assert_eq!(index["releases"][0]["version"], "0.3.1");
    assert_eq!(aws.json("updates/stable.json")["version"], "0.3.1");
}

#[test]
fn unsafe_publishes_are_refused_and_leave_the_feed_alone() {
    let fixture = Fixture::new("refused");
    let (old, _) = fixture.stage("0.2.9", "darwin-aarch64", 400);
    let (current, _) = fixture.stage("0.3.0", "darwin-aarch64", 400);
    let aws = FakeAws::new();
    let cdn = FakeCdn(&aws);
    publisher(&aws, &cdn).publish(&current).unwrap();
    let feed = aws.object("updates/stable.json").unwrap();

    // Downgrade.
    let e = publisher(&aws, &cdn).publish(&old).unwrap_err();
    assert!(format!("{e:#}").contains("downgrade"), "{e:#}");
    assert!(aws.object(LOCK_KEY).is_none());

    // A newer version for another platform would strand darwin-aarch64.
    let (windows, _) = fixture.stage("0.3.1", "windows-x86_64", 400);
    assert!(
        format!("{:#}", publisher(&aws, &cdn).publish(&windows).unwrap_err()).contains("strand")
    );

    // A rebuilt package (another signature) for a published version.
    let rebuilt_fixture = Fixture::new("rebuilt");
    let (rebuilt, _) = rebuilt_fixture.stage("0.3.0", "darwin-aarch64", 400);
    assert!(publisher(&aws, &cdn).publish(&rebuilt).is_err());

    // Something else already stored under a versioned key.
    let (next, _) = fixture.stage("0.3.1", "darwin-aarch64", 400);
    aws.objects.borrow_mut().insert(
        format!("updates/releases/0.3.1/{}", next.artifact.filename),
        vec![0; 400],
    );
    let e = publisher(&aws, &cdn).publish(&next).unwrap_err();
    assert!(
        format!("{e:#}").contains("immutable release file conflict"),
        "{e:#}"
    );

    // Another publisher holds the lock: refused, and its lock is left alone.
    aws.objects
        .borrow_mut()
        .insert(LOCK_KEY.into(), b"{}".to_vec());
    let e = publisher(&aws, &cdn).publish(&current).unwrap_err();
    assert!(format!("{e:#}").contains("another publish"), "{e:#}");
    assert!(aws.object(LOCK_KEY).is_some());
    aws.objects.borrow_mut().remove(LOCK_KEY);

    assert_eq!(aws.object("updates/stable.json").unwrap(), feed);

    // The wrong AWS account is refused before anything else.
    let other = FakeAws {
        account: "999".into(),
        ..FakeAws::new()
    };
    assert!(check_identity(&other, &deployment()).is_err());
}

#[test]
fn a_failed_part_aborts_the_upload_and_a_retry_completes() {
    let fixture = Fixture::new("abort");
    let (staged, _) = fixture.stage("0.3.0", "darwin-aarch64", 2000);
    let aws = FakeAws::new();
    let cdn = FakeCdn(&aws);
    *aws.fail.borrow_mut() = Some("upload-part".into());
    assert!(publisher(&aws, &cdn).publish(&staged).is_err());
    assert_eq!(aws.calls_of("abort-multipart-upload"), 1);
    assert!(aws.object("updates/stable.json").is_none());
    assert!(aws.object(LOCK_KEY).is_none());
    publisher(&aws, &cdn).publish(&staged).unwrap();
    assert_eq!(aws.json("updates/stable.json")["version"], "0.3.0");
}

#[test]
fn staged_artifacts_must_match_their_package_and_notes() {
    let fixture = Fixture::new("staged");
    let (staged, log) = fixture.stage("0.3.0", "darwin-aarch64", 300);
    // Notes edited after the build.
    let mut edited = log.clone();
    edited.releases[0].title = "Renamed".into();
    assert!(verify_staged(&staged.artifact_path, &edited, &fixture.public_key).is_err());
    // Another key.
    let other = Fixture::new("staged-other");
    assert!(verify_staged(&staged.artifact_path, &log, &other.public_key).is_err());
    // A package changed after signing.
    let mut bytes = fs::read(&staged.package).unwrap();
    bytes[0] ^= 1;
    fs::write(&staged.package, bytes).unwrap();
    assert!(verify_staged(&staged.artifact_path, &log, &fixture.public_key).is_err());
}

#[test]
fn ranged_digests_retry_and_report_progress() {
    let data: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
    let mut failures = 2;
    let mut seen = Vec::new();
    let hash = ranged_digest(
        1000,
        64,
        |s, e| {
            if s == 128 && failures > 0 {
                failures -= 1;
                return Ok(vec![0; 3]);
            }
            Ok(data[s as usize..=e as usize].to_vec())
        },
        |p| seen.push(p),
    )
    .unwrap();
    assert_eq!(hash, crate::package::hex(&Sha256::digest(&data)));
    assert_eq!(seen, [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
    assert!(ranged_digest(1000, 64, |_, _| Ok(vec![0; 3]), |_| {}).is_err());
}
