//! Pictures to think with (D110, D111): concept images for a campaign step's outline,
//! generated with OpenAI's image models on the key the assistant's Settings hold, written
//! to the campaign's `outlines/images/` and listed in the outline, so ideas can be seen and
//! iterated on: a new picture can start from earlier ones or from the game's art. Releases
//! leave them out with the outlines. The request runs on a thread of its own (a picture
//! takes up to two minutes); the shell writes the files and the outline's undo step.

use base64::Engine;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// The image models offered, the default first.
pub const MODELS: [&str; 3] = [
    "gpt-image-2.5-sunburst",
    "gpt-image-2.5-flare",
    "gpt-image-2",
];
/// OpenAI's images API.
pub const API: &str = "https://api.openai.com/v1/images";
const SIZE: &str = "1536x1024";
const QUALITY: &str = "medium";
/// The most pictures one call makes.
pub const MOST: u32 = 4;
/// The width of the preview the assistant sees of one picture.
const PREVIEW: u32 = 1024;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImage {
    /// The campaign: its id or its package path.
    pub campaign: String,
    /// The step whose outline gets the pictures: its id or its index.
    pub step: Value,
    /// What to paint, in full: subject, composition, light, mood, and the game's style
    /// (skill canopy-outlines has the house style to end every prompt with).
    pub prompt: String,
    /// What the picture shows or is for, in a few words (the outline shows it).
    pub caption: Option<String>,
    /// Pictures to start from: files of the campaign's outlines/images (any step's) or
    /// image asset ids (`asset.interface.campaign.vanguard.…`). With them the model edits
    /// and varies; without them it paints anew.
    #[serde(default)]
    pub references: Vec<String>,
    /// gpt-image-2.5-sunburst (the default: the best), gpt-image-2.5-flare (fast, for
    /// quick variations) or gpt-image-2.
    pub model: Option<String>,
    /// `1536x1024` (the default, landscape), `1024x1024`, `1024x1536`, or larger such as
    /// `2048x1152` for a finished key frame.
    pub size: Option<String>,
    /// low, medium (the default) or high.
    pub quality: Option<String>,
    /// How many takes, 1 (the default) to 4.
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewImage {
    /// The campaign whose outlines/images holds `file`.
    pub campaign: Option<String>,
    /// A picture in the campaign's outlines/images.
    pub file: Option<String>,
    /// Or an image asset of the game (story pages, mission art, portraits, icons).
    pub asset: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Call {
    Generate(GenerateImage),
    View(ViewImage),
}

/// One request to the API, ready to send.
#[derive(Debug, Clone)]
pub struct Request {
    pub model: String,
    pub prompt: String,
    pub size: String,
    pub quality: String,
    pub count: u32,
    /// The pictures to start from: their bytes.
    pub references: Vec<Vec<u8>>,
}

impl Request {
    /// The request `args` asks for, its references read.
    pub fn of(args: &GenerateImage, references: Vec<Vec<u8>>) -> Result<Self, String> {
        let model = args.model.clone().unwrap_or_else(|| MODELS[0].to_string());
        if !MODELS.contains(&model.as_str()) {
            return Err(format!(
                "Unknown image model {model}: use one of {}",
                MODELS.join(", ")
            ));
        }
        let quality = args.quality.clone().unwrap_or_else(|| QUALITY.to_string());
        if !["low", "medium", "high"].contains(&quality.as_str()) {
            return Err(format!("quality is low, medium or high, not {quality}"));
        }
        let size = args.size.clone().unwrap_or_else(|| SIZE.to_string());
        let valid = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            .is_some_and(|(w, h)| (256..=4096).contains(&w) && (256..=4096).contains(&h));
        if !valid {
            return Err(format!("size is WIDTHxHEIGHT, such as {SIZE}, not {size}"));
        }
        if args.prompt.trim().is_empty() {
            return Err("The prompt is empty".into());
        }
        Ok(Self {
            model,
            prompt: args.prompt.clone(),
            size,
            quality,
            count: args.count.unwrap_or(1).clamp(1, MOST),
            references,
        })
    }

    /// The endpoint: an edit when it starts from pictures, else a generation.
    pub fn endpoint(&self) -> &'static str {
        if self.references.is_empty() {
            "generations"
        } else {
            "edits"
        }
    }

    /// The JSON body.
    pub fn body(&self) -> Value {
        let mut body = json!({
            "model": self.model,
            "prompt": self.prompt,
            "size": self.size,
            "quality": self.quality,
            "n": self.count,
            "background": "opaque",
            "output_format": "webp",
            "output_compression": 90,
            "moderation": "auto",
        });
        if !self.references.is_empty() {
            body["images"] = self
                .references
                .iter()
                .map(|bytes| json!({ "image_url": data_url(bytes) }))
                .collect();
        }
        body
    }
}

/// `bytes` as a data URL, its type from its first bytes.
fn data_url(bytes: &[u8]) -> String {
    let mime = match image::guess_format(bytes) {
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::WebP) => "image/webp",
        _ => "image/png",
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// The pictures of an answer, or why there are none.
pub fn pictures(answer: &Value) -> Result<Vec<Vec<u8>>, String> {
    let decoded: Vec<Vec<u8>> = answer["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["b64_json"].as_str())
        .filter_map(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok())
        .collect();
    if decoded.is_empty() {
        return Err("The image model answered with no picture.".into());
    }
    Ok(decoded)
}

/// Sends `request` with `key` to `api` and waits for its pictures. Never retried: a
/// picture costs money, and an interrupted request may still have been made.
pub fn generate(api: &str, key: &str, request: &Request) -> Result<Vec<Vec<u8>>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let tls = canopy_net::tls::client_config().map_err(|e| e.to_string())?;
        let local = api.starts_with("http://127.0.0.1");
        let client = reqwest::Client::builder()
            .use_preconfigured_tls((*tls).clone())
            .https_only(!local)
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(420))
            .user_agent(concat!("UnderTheCanopy-editor/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .post(format!("{api}/{}", request.endpoint()))
            .bearer_auth(key)
            .json(&request.body())
            .send()
            .await
            .map_err(|_| {
                "The connection to OpenAI broke; whether the picture was made is unknown, so it was not retried.".to_string()
            })?;
        let status = response.status();
        let answer: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let reason = answer["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .replace(key, "[key]");
            return Err(format!("OpenAI answered {status}: {reason}"));
        }
        pictures(&answer)
    })
}

/// `pictures` side by side (two to a row), at most [`PREVIEW`] wide, as a JPEG in base64:
/// what the assistant sees of them.
pub fn preview(pictures: &[Vec<u8>]) -> Option<String> {
    let decoded: Vec<image::DynamicImage> = pictures
        .iter()
        .filter_map(|bytes| image::load_from_memory(bytes).ok())
        .collect();
    let first = decoded.first()?;
    let columns = decoded.len().min(2) as u32;
    let rows = (decoded.len() as u32).div_ceil(columns);
    let cell_w = PREVIEW / columns;
    let cell_h = (first.height() as f32 * cell_w as f32 / first.width().max(1) as f32) as u32;
    let mut sheet = image::RgbImage::new(cell_w * columns, cell_h.max(1) * rows);
    for (i, picture) in decoded.iter().enumerate() {
        let small = picture
            .resize_exact(cell_w, cell_h.max(1), image::imageops::FilterType::Triangle)
            .to_rgb8();
        let (x, y) = (i as u32 % columns * cell_w, i as u32 / columns * cell_h);
        image::imageops::replace(&mut sheet, &small, x.into(), y.into());
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 82)
        .encode_image(&sheet)
        .ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(jpeg))
}

/// A file name for a new picture of step `step`: `<step>-<words>-<n>.<ext>`, free in
/// `taken`.
pub fn file_name(
    step: &str,
    caption: Option<&str>,
    ext: &str,
    taken: &dyn Fn(&str) -> bool,
) -> String {
    let words: String = crate::campaign::slug(caption.unwrap_or("idea"))
        .split('-')
        .filter(|w| !w.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("-");
    let stem = if words.is_empty() {
        step.to_string()
    } else {
        format!("{step}-{words}")
    };
    (1..)
        .map(|n| format!("{stem}-{n}.{ext}"))
        .find(|name| !taken(name))
        .unwrap_or_default()
}

/// The extension that fits `bytes`.
pub fn extension(bytes: &[u8]) -> &'static str {
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::WebP) => "webp",
        _ => "png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(value: Value) -> GenerateImage {
        serde_json::from_value(value).unwrap()
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(64, 32)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn a_request_paints_anew_or_edits_from_its_references() {
        let a = args(json!({"campaign": "c", "step": "yard", "prompt": "A yard at dusk"}));
        let request = Request::of(&a, Vec::new()).unwrap();
        assert_eq!(request.endpoint(), "generations");
        let body = request.body();
        assert_eq!(body["model"], MODELS[0]);
        assert_eq!(
            (body["size"].as_str(), body["n"].as_u64()),
            (Some(SIZE), Some(1))
        );
        assert!(body.get("images").is_none());
        let edit = Request::of(&a, vec![png()]).unwrap();
        assert_eq!(edit.endpoint(), "edits");
        let url = edit.body()["images"][0]["image_url"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(url.starts_with("data:image/png;base64,"));
        for bad in [
            json!({"campaign": "c", "step": 1, "prompt": "x", "model": "dall-e-2"}),
            json!({"campaign": "c", "step": 1, "prompt": "x", "size": "huge"}),
            json!({"campaign": "c", "step": 1, "prompt": " "}),
        ] {
            assert!(Request::of(&args(bad), Vec::new()).is_err());
        }
    }

    #[test]
    fn an_answer_gives_its_pictures_and_a_preview_of_them() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(png());
        let answer = json!({"data": [{"b64_json": b64}, {"b64_json": b64}, {"b64_json": b64}]});
        let pictures = pictures(&answer).unwrap();
        assert_eq!(pictures.len(), 3);
        assert_eq!(extension(&pictures[0]), "png");
        let sheet = base64::engine::general_purpose::STANDARD
            .decode(preview(&pictures).unwrap())
            .unwrap();
        let sheet = image::load_from_memory(&sheet).unwrap();
        // Two to a row, two rows.
        assert_eq!(
            (sheet.width(), sheet.height()),
            (PREVIEW, PREVIEW / 2 / 2 * 2)
        );
        assert!(super::pictures(&json!({"data": []})).is_err());
    }

    #[test]
    fn new_pictures_get_free_names_from_their_step_and_caption() {
        let taken = |n: &str| n == "yard-the-gate-at-dusk-1.webp";
        assert_eq!(
            file_name("yard", Some("The gate at dusk!"), "webp", &taken),
            "yard-the-gate-at-dusk-2.webp"
        );
        assert_eq!(
            file_name("yard", None, "png", &|_| false),
            "yard-idea-1.png"
        );
    }
}
