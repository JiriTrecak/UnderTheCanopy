//! The shell's side of outline pictures (`crate::images`, D111): a call to paint one reads
//! its references and the key, and runs on a thread of its own; when its pictures arrive
//! the shell writes them to the campaign's `outlines/images/` and adds them to the step's
//! outline as one undo step, then answers with a preview. Looking at a picture answers at
//! once.

use super::*;
use crate::campaign;
use crate::images::{self, GenerateImage, ViewImage};
use canopy_content::outline::IMAGES_DIR;
use serde_json::json;
use std::sync::mpsc;

/// A picture being painted, and the call waiting for it.
pub(crate) struct ImageJob {
    args: GenerateImage,
    /// The campaign's folder and the step's id.
    root: String,
    step: String,
    pictures: mpsc::Receiver<Result<Vec<Vec<u8>>, String>>,
    reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
}

impl Shell {
    /// The disk folder of a campaign's outline pictures.
    fn pictures_folder(&self, root: &str) -> Result<std::path::PathBuf, String> {
        let package = self.package.as_ref().map_err(Clone::clone)?;
        Ok(package.root.join(root).join(IMAGES_DIR))
    }

    /// The bytes of a reference: a picture of the campaign's outlines/images, or an image
    /// asset's picture.
    fn reference(&self, root: &str, name: &str) -> Result<Vec<u8>, String> {
        let file = if name.starts_with("asset.") {
            let entry = self
                .index
                .get(name)
                .ok_or_else(|| format!("No asset {name}"))?;
            let package = self.package.as_ref().map_err(Clone::clone)?;
            let folder = package.root.join(&entry.document);
            folder
                .parent()
                .and_then(crate::campaign::art::picture_file)
                .ok_or_else(|| format!("{name} has no picture"))?
        } else if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            return Err(format!("{name} is not a picture of outlines/images"));
        } else {
            self.pictures_folder(root)?.join(name)
        };
        std::fs::read(&file).map_err(|e| format!("{name} does not read: {e}"))
    }

    /// Starts painting the pictures `args` asks for.
    pub(super) fn start_image(
        &mut self,
        args: GenerateImage,
        reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
    ) {
        let started = (|| {
            let root = self.campaign_root(&args.campaign)?;
            let value = self.campaign_value(&root)?;
            let index = Self::step_index(&value, &args.step)?;
            let step = campaign::text(&campaign::steps(&value)[index], "id").to_string();
            let references = args
                .references
                .iter()
                .map(|r| self.reference(&root, r))
                .collect::<Result<Vec<_>, _>>()?;
            let request = images::Request::of(&args, references)?;
            let key = crate::assistant::read_key(
                self.user.as_deref(),
                crate::assistant::api::Provider::OpenAi,
            );
            let key = key
                .value()
                .ok_or("No OpenAI key: enter one in the assistant's Settings (OpenAI), or set OPENAI_API_KEY.")?
                .to_string();
            let (tx, pictures) = mpsc::channel();
            std::thread::Builder::new()
                .name("outline picture".into())
                .spawn(move || {
                    let _ = tx.send(images::generate(images::API, &key, &request));
                })
                .map_err(|e| e.to_string())?;
            Ok::<_, String>((root, step, pictures))
        })();
        match started {
            Ok((root, step, pictures)) => self.image_jobs.push(ImageJob {
                args,
                root,
                step,
                pictures,
                reply,
            }),
            Err(error) => {
                let _ = reply.send(Err(error));
            }
        }
    }

    /// Finishes the pictures that arrived: their files, the outline's step, the answer.
    pub(super) fn poll_images(&mut self) {
        let mut i = 0;
        while i < self.image_jobs.len() {
            let arrived = match self.image_jobs[i].pictures.try_recv() {
                Ok(arrived) => arrived,
                Err(mpsc::TryRecvError::Empty) => {
                    i += 1;
                    continue;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("The picture's request stopped without an answer.".into())
                }
            };
            let job = self.image_jobs.remove(i);
            let answer = arrived.and_then(|pictures| self.keep_pictures(&job, &pictures));
            let _ = job.reply.send(answer);
        }
    }

    /// Writes `pictures` and lists them in the step's outline.
    fn keep_pictures(&mut self, job: &ImageJob, pictures: &[Vec<u8>]) -> Result<Value, String> {
        let folder = self.pictures_folder(&job.root)?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let mut files = Vec::new();
        for bytes in pictures {
            let name = images::file_name(
                &job.step,
                job.args.caption.as_deref(),
                images::extension(bytes),
                &|n| folder.join(n).exists(),
            );
            std::fs::write(folder.join(&name), bytes).map_err(|e| e.to_string())?;
            files.push(name);
        }
        let document = campaign::outline_document(&job.root, &job.step);
        let held = self
            .store
            .as_mut()
            .and_then(|s| s.get(&document).ok().flatten().cloned());
        let request = images::Request::of(&job.args, Vec::new())?;
        let entries: Vec<Value> = files
            .iter()
            .map(|file| {
                let mut entry =
                    json!({ "file": file, "prompt": job.args.prompt, "model": request.model });
                if let Some(caption) = &job.args.caption {
                    entry["caption"] = json!(caption);
                }
                if !job.args.references.is_empty() {
                    entry["references"] = json!(job.args.references);
                }
                entry
            })
            .collect();
        let commands = match &held {
            Some(outline) => {
                let at = outline["images"].as_array().map_or(0, Vec::len);
                if outline.get("images").is_some() {
                    entries
                        .iter()
                        .enumerate()
                        .map(|(n, entry)| Command::Insert {
                            document: document.clone(),
                            pointer: "/images".into(),
                            index: at + n,
                            value: entry.clone(),
                        })
                        .collect()
                } else {
                    vec![Command::Set {
                        document: document.clone(),
                        pointer: "/images".into(),
                        value: json!(entries),
                        at: None,
                    }]
                }
            }
            None => vec![Command::Create {
                document: document.clone(),
                value: json!({ "images": entries }),
            }],
        };
        let label = match &job.args.caption {
            Some(caption) => format!("Picture: {caption}"),
            None => "Outline picture".into(),
        };
        self.run_campaign(&job.root, &label, commands)?;
        let mut answer = json!({
            "files": files,
            "folder": format!("{}/{IMAGES_DIR}", job.root),
            "outline": document,
            "model": request.model,
            "note": "Written at once; listed in the outline as an undo step. A picture no saved outline names is removed on save.",
        });
        if let Some(preview) = images::preview(pictures) {
            answer["jpeg"] = json!(preview);
        }
        Ok(answer)
    }

    /// Looks at a picture: an outline picture or an image asset.
    pub(super) fn view_image(&mut self, args: ViewImage) -> Result<Value, String> {
        let (bytes, about) = match (&args.file, &args.asset) {
            (Some(file), _) => {
                let campaign = args
                    .campaign
                    .as_deref()
                    .ok_or("Name the campaign whose outlines/images holds the file")?;
                let root = self.campaign_root(campaign)?;
                let bytes = self.reference(&root, file)?;
                // What the outlines say of it, and which step shows it.
                let mut about = Value::Null;
                let value = self.campaign_value(&root)?;
                for step in campaign::steps(&value) {
                    let id = campaign::text(step, "id");
                    let document = campaign::outline_document(&root, id);
                    let outline = self
                        .store
                        .as_mut()
                        .and_then(|s| s.get(&document).ok().flatten().cloned());
                    if let Some(entry) = outline
                        .as_ref()
                        .and_then(|o| o["images"].as_array())
                        .and_then(|list| list.iter().find(|p| p["file"] == file.as_str()))
                    {
                        about = json!({ "step": id, "picture": entry });
                        break;
                    }
                }
                (bytes, about)
            }
            (None, Some(asset)) => {
                let name = self.index.get(asset).map(|e| e.name.clone());
                (
                    self.reference("", asset)?,
                    json!({ "asset": asset, "name": name }),
                )
            }
            (None, None) => return Err("Name a file (with its campaign) or an asset".into()),
        };
        let preview = images::preview(&[bytes]).ok_or("The picture does not decode")?;
        let mut answer = json!({ "jpeg": preview });
        if !about.is_null() {
            answer["about"] = about;
        }
        Ok(answer)
    }
}
