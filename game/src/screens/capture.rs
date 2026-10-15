//! Offscreen captures of every screen: `canopy capture-screens --out <folder>
//! [--size <w>x<h>] [--scale <s>]` (docs/captures/m4-screens).
//!
//! The real [`Game`] runs headless: the same flow, models, layouts and pack images as in
//! the window, driven through its intents and actions; matches really load and run on
//! the simulation thread and their frame renders behind the UI. Saves go to a temporary
//! user folder. Multiplayer shows a real server when `CANOPY_MATCH_SERVER` names one (a
//! local copy of the old server): the capture opens a second client's lobby so the list
//! has a room, then joins it. No shipped map is a mission, so the mission HUD captures
//! feed a sample mission state (marked `-sample` in their names).

use super::{
    Ctx, Game, Intent, Overlay, Screen,
    match_ui::{MenuPage, MissionHud},
};
use crate::{paths::Paths, settings::Settings};
use anyhow::{Context, Result};
use canopy_render::{DeviceOptions, Gpu, Renderer, capture::CAPTURE_FORMAT};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// An sRGB target with a non-sRGB view for the UI, read back as RGBA8.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    ui_view: wgpu::TextureView,
    buffer: wgpu::Buffer,
    size: [u32; 2],
    padded_row: u32,
}

impl Target {
    fn new(gpu: &Gpu, size: [u32; 2]) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen capture"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAPTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[CAPTURE_FORMAT.remove_srgb_suffix()],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let ui_view = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(CAPTURE_FORMAT.remove_srgb_suffix()),
            ..Default::default()
        });
        let padded_row = (size[0] * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen capture readback"),
            size: u64::from(padded_row) * u64::from(size[1]),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            texture,
            view,
            ui_view,
            buffer,
            size,
            padded_row,
        }
    }

    fn read(&self, gpu: &Gpu) -> Result<Vec<u8>> {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.size[1]),
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        self.buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let mut out = Vec::with_capacity((self.size[0] * self.size[1] * 4) as usize);
        {
            let mapped = self.buffer.get_mapped_range(..)?;
            for row in mapped.chunks_exact(self.padded_row as usize) {
                out.extend_from_slice(&row[..self.size[0] as usize * 4]);
            }
        }
        self.buffer.unmap();
        Ok(out)
    }
}

struct Rig {
    gpu: Gpu,
    renderer: Renderer,
    target: Target,
    game: Game,
    size: [u32; 2],
    scale: f32,
    out: PathBuf,
    written: Vec<String>,
}

impl Rig {
    fn frame(&mut self) {
        let mut ctx = Ctx {
            gpu: &self.gpu,
            renderer: &mut self.renderer,
            window: None,
            size: self.size,
            scale: self.scale,
        };
        self.game.update(&mut ctx);
    }

    /// Runs frames for `seconds` of wall time (at least three, so what is placed from
    /// last frame's boxes settles).
    fn settle(&mut self, seconds: f32) {
        let until = Instant::now() + Duration::from_secs_f32(seconds);
        let mut n = 0;
        while n < 3 || Instant::now() < until {
            self.frame();
            std::thread::sleep(Duration::from_millis(16));
            n += 1;
        }
    }

    fn until(&mut self, what: &str, limit: Duration, done: impl Fn(&Game) -> bool) -> Result<()> {
        let start = Instant::now();
        while !done(&self.game) {
            anyhow::ensure!(start.elapsed() < limit, "timed out waiting for {what}");
            self.frame();
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    /// Renders the last prepared frame and writes it.
    fn write(&mut self, name: &str) -> Result<()> {
        let (view, ui_view) = (self.target.view.clone(), self.target.ui_view.clone());
        let mut ctx = Ctx {
            gpu: &self.gpu,
            renderer: &mut self.renderer,
            window: None,
            size: self.size,
            scale: self.scale,
        };
        self.game.render(&mut ctx, &view, &ui_view);
        let rgba = self.target.read(&self.gpu)?;
        let path = self.out.join(format!("{name}.png"));
        image::save_buffer(
            &path,
            &rgba,
            self.size[0],
            self.size[1],
            image::ColorType::Rgba8,
        )
        .with_context(|| path.display().to_string())?;
        println!("wrote {}", path.display());
        self.written.push(name.to_string());
        Ok(())
    }

    fn shoot(&mut self, name: &str) -> Result<()> {
        self.frame();
        self.write(name)
    }

    /// Shoots after a UI tweak the game's own frame would undo (sample overlays).
    fn shoot_as_is(&mut self, name: &str) -> Result<()> {
        self.game.prepare_ui(&self.gpu, &mut self.renderer, 1.0);
        self.write(name)
    }

    fn click(&mut self, name: &str) {
        let action = canopy_ui::Action {
            node: None,
            path: None,
            name: Some(name.into()),
            index: None,
            kind: canopy_ui::ActionKind::Click {
                button: canopy_ui::PointerButton::Primary,
                modifiers: canopy_ui::Modifiers::default(),
                count: 1,
            },
        };
        let mut ctx = Ctx {
            gpu: &self.gpu,
            renderer: &mut self.renderer,
            window: None,
            size: self.size,
            scale: self.scale,
        };
        self.game.action(&action, &mut ctx);
    }
}

fn sample_mission(portrait: canopy_content::Id) -> canopy_sim::view::MissionView {
    use canopy_sim::mission::state::{Dialogue, ObjectiveState};
    let mut objectives = std::collections::BTreeMap::new();
    objectives.insert("gather".to_string(), ObjectiveState::Completed);
    objectives.insert("scouts".to_string(), ObjectiveState::Active);
    objectives.insert("hollow".to_string(), ObjectiveState::Active);
    canopy_sim::view::MissionView {
        objective: "Find the lost scouts beyond the river".into(),
        objectives,
        dialogue: Some(Dialogue {
            id: 1,
            actor: None,
            speaker: "Marshal".into(),
            portrait,
            text: "The scouts went north at dawn and never came back. Gather what amber you can, then follow the river.".into(),
            duration_ticks: 200,
            cinematic: false,
            remaining: 0,
            until: 400,
        }),
        scene: None,
        paused: false,
        error: None,
    }
}

fn objective(id: &str, title: &str, optional: bool) -> canopy_content::map::Objective {
    canopy_content::map::Objective {
        id: id.into(),
        title: title.into(),
        description: String::new(),
        optional,
    }
}

/// Renders every screen to PNGs in `out`.
pub fn run(paths: &Paths, out: &Path, size: [u32; 2], scale: f32) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
    let user = std::env::temp_dir().join(format!("canopy-capture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&user);
    std::fs::create_dir_all(&user)?;
    let paths = Paths {
        data: paths.data.clone(),
        user: user.clone(),
    };
    let look = canopy_scene::Look::load(&paths.data, canopy_content::map::BIOMES[0])?;
    let environment = canopy_render::Environment::new(&look.lighting, &look.biome, 12.0, None);
    let renderer = Renderer::new(&gpu, CAPTURE_FORMAT, size, environment);
    let settings = Settings {
        player_name: "Jiri".into(),
        ..Settings::default()
    };
    let game = Game::new(paths.clone(), settings, &gpu, None)?;
    let target = Target::new(&gpu, size);
    let mut rig = Rig {
        gpu,
        renderer,
        target,
        game,
        size,
        scale,
        out: out.to_path_buf(),
        written: Vec::new(),
    };
    rig.frame();
    rig.shoot("boot")?;
    rig.until("boot", Duration::from_secs(60), Game::booted)?;
    rig.settle(1.2);
    rig.shoot("main-menu")?;
    settings_tabs(&mut rig)?;

    // Campaign and missions.
    rig.game.apply(Intent::Open(Screen::Campaign));
    rig.settle(0.4);
    rig.shoot("campaign")?;
    let campaign = rig
        .game
        .data
        .as_ref()
        .and_then(|d| d.content.rules.campaigns.keys().next().cloned());
    if let Some(campaign) = campaign {
        rig.game.apply(Intent::Open(Screen::Missions { campaign }));
        rig.settle(0.3);
        rig.shoot("missions")?;
        rig.game.apply(Intent::Back);
    }
    rig.game.apply(Intent::Back);

    skirmish(&mut rig)?;
    multiplayer(&mut rig)?;
    a_match(&mut rig)?;

    // Loading a save from the main menu.
    rig.game.open_saves();
    rig.settle(0.2);
    rig.shoot("saves-main-menu")?;
    let _ = std::fs::remove_dir_all(&user);
    println!("{} captures", rig.written.len());
    Ok(())
}

fn settings_tabs(rig: &mut Rig) -> Result<()> {
    use super::settings_ui::Tab;
    rig.game.open_settings();
    rig.settle(0.2);
    rig.shoot("settings-graphics")?;
    for (tab, name) in [
        (Tab::Audio, "settings-audio"),
        (Tab::Interface, "settings-interface"),
    ] {
        rig.game.settings_ui.tab = tab;
        rig.settle(0.1);
        rig.shoot(name)?;
    }
    // A refused rebinding (the key is taken), then one waiting for a key.
    rig.game.settings_ui.tab = Tab::Keys;
    let catalog = crate::settings::keys::catalog();
    rig.game.settings_ui.capturing = Some("interface.gameMenu".into());
    let _ = rig.game.settings_ui.capture(
        &mut rig.game.settings,
        &catalog,
        crate::settings::Chord::parse("KeyR"),
    );
    rig.game.settings_ui.capturing = Some("interface.toggleHud".into());
    rig.settle(0.1);
    rig.shoot("settings-keys")?;
    rig.game.settings_ui.capturing = None;
    rig.game.settings_ui.tab = Tab::Network;
    rig.settle(0.1);
    rig.shoot("settings-network")?;
    rig.game.settings_ui.tab = Tab::Graphics;
    rig.game.close_settings();
    Ok(())
}

fn skirmish(rig: &mut Rig) -> Result<()> {
    // The largest map, a team game with the player on the second start.
    rig.game.apply(Intent::Open(Screen::Skirmish));
    let data = rig.game.data.clone().context("booted")?;
    let maps = super::skirmish::Skirmish::maps(&data);
    let largest = maps
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.starts.len())
        .map_or(0, |(i, _)| i);
    rig.game.skirmish.choose_map(&data, largest);
    if rig.game.skirmish.slots.len() >= 4 {
        rig.game.skirmish.set_human(1, true);
        for (i, team) in [(0, 0), (1, 0), (2, 1), (3, 1)] {
            rig.game.skirmish.slots[i].team = Some(team);
        }
    }
    rig.settle(0.4);
    rig.shoot("skirmish")?;
    // A dropdown open.
    if let Some(id) = rig.game.host.ui.find("skirmish/players/0/race")
        && let Some(r) = rig.game.host.ui.rect(id)
    {
        let (x, y) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        rig.game
            .host
            .ui
            .input(&canopy_ui::InputEvent::PointerMoved { x, y });
        for pressed in [true, false] {
            rig.game
                .host
                .ui
                .input(&canopy_ui::InputEvent::PointerButton {
                    button: canopy_ui::PointerButton::Primary,
                    pressed,
                });
        }
        rig.settle(0.1);
        rig.shoot("skirmish-race-select")?;
        for pressed in [true, false] {
            rig.game.host.ui.input(&canopy_ui::InputEvent::Key {
                key: canopy_ui::Key::Escape,
                pressed,
                repeat: false,
            });
        }
    }
    rig.game.apply(Intent::Back);
    Ok(())
}

/// The multiplayer list and a lobby against a real server, when one is named.
fn multiplayer(rig: &mut Rig) -> Result<()> {
    use canopy_net::wire::CreateRoom;
    let server = std::env::var(canopy_net::server::ENV).ok();
    let mut other = None;
    if let Some(text) = &server {
        let address = canopy_net::ServerAddress::parse(text)?;
        let net = canopy_net::Net::new(address)?;
        let data = rig.game.data.clone().context("booted")?;
        let map = data
            .skirmish_maps()
            .max_by_key(|m| m.starts.len())
            .context("a map")?;
        let bytes = std::fs::read(&map.file.path)?;
        let joined = net
            .create_room(&CreateRoom {
                name: "Bo's lobby".into(),
                map_id: map.id().into(),
                map_revision: super::multiplayer::map_revision(&bytes),
                slot_count: 3,
                guest_name: "Bo".into(),
            })
            .wait()?;
        let session = net.enter(&joined);
        other = Some((net, session));
    }
    rig.game.apply(Intent::Open(Screen::Multiplayer));
    rig.settle(if server.is_some() { 1.5 } else { 3.0 });
    rig.shoot("multiplayer")?;
    rig.game.mp.tab = super::multiplayer::Tab::Create;
    rig.settle(0.2);
    rig.shoot("multiplayer-create")?;
    rig.game.mp.tab = super::multiplayer::Tab::Find;
    rig.settle(0.2);
    if server.is_some() && !rig.game.mp.rooms.is_empty() {
        let data = rig.game.data.clone().context("booted")?;
        rig.game
            .mp
            .go(&data.skirmish_maps().collect::<Vec<_>>(), "Jiri");
        rig.until("the lobby", Duration::from_secs(20), |g| {
            *g.flow.screen() == Screen::Lobby
        })?;
        rig.settle(2.5);
        rig.shoot("lobby")?;
        rig.game.apply(Intent::Back);
        rig.settle(0.3);
    }
    if let Some((_net, mut session)) = other {
        session.close();
    }
    rig.game.apply(Intent::Back);
    rig.settle(0.2);
    Ok(())
}

/// A skirmish match: loading, the match, overlays, its menu, saves, a confirmation and
/// the result.
fn a_match(rig: &mut Rig) -> Result<()> {
    let data = rig.game.data.clone().context("booted")?;
    rig.game.apply(Intent::Open(Screen::Skirmish));
    rig.game.skirmish.choose_map(&data, 0);
    rig.game.skirmish.set_human(0, true);
    for slot in &mut rig.game.skirmish.slots {
        slot.team = None;
    }
    rig.game.start_skirmish();
    rig.settle(0.15);
    rig.shoot("loading")?;
    rig.until("the match", Duration::from_secs(180), |g| g.flow.in_match())?;
    rig.settle(3.0);
    rig.shoot("match")?;
    // The play HUD with the player's units selected and kept as control group 1.
    let mine: Vec<u32> = rig.game.playing.as_ref().map_or_else(Vec::new, |p| {
        let k = p.game.play.knowledge();
        k.view
            .entities
            .iter()
            .filter(|e| k.controlled(e) && k.is_unit(e))
            .map(|e| e.id().0)
            .collect()
    });
    anyhow::ensure!(!mine.is_empty(), "the player has units");
    if let Some(p) = &mut rig.game.playing {
        p.game.play.select(mine.clone(), None);
        p.game.play.groups.assign(0, &mine);
    }
    rig.settle(0.3);
    rig.shoot("match-selection")?;
    if let Some(playing) = &mut rig.game.playing {
        for (player, name, text) in [
            (1, "Bo", "glhf!"),
            (0, "Jiri", "you too, see you at the river"),
        ] {
            playing.hear(&canopy_net::wire::ChatMessage {
                name: name.into(),
                player: Some(player),
                text: text.into(),
            });
        }
    }
    rig.settle(0.1);
    rig.shoot("match-chat")?;

    // The mission HUD with a sample mission state.
    let mut hud = MissionHud::new(vec![
        objective("gather", "Gather 200 amber", false),
        objective("scouts", "Find the lost scouts", false),
        objective("hollow", "Clear the beetle hollow", true),
    ]);
    // The portrait of the content's default starting hero.
    let rules = &data.content.rules;
    let hero = rules
        .race(&rules.default_race)
        .map_or(&rules.starting_setup, |p| p.starting_setup)
        .hero
        .as_ref()
        .map(|h| h.default.clone())
        .context("a starting hero")?;
    let mission = sample_mission(hero);
    let view_h = rig.size[1] as f32 / rig.scale;
    rig.frame();
    let hud_top = rig
        .game
        .host
        .ui
        .find("hud/dock")
        .and_then(|d| rig.game.host.ui.rect(d))
        .map(|r| r.y / rig.scale);
    hud.hud_top = hud_top;
    rig.game.host.ui.show("mission-hud", true);
    for _ in 0..3 {
        hud.show(
            &mut rig.game.host,
            &rig.gpu,
            &data,
            Some(&mission),
            0.016,
            view_h,
        );
        rig.game.prepare_ui(&rig.gpu, &mut rig.renderer, 1.0);
    }
    rig.shoot_as_is("mission-hud-sample")?;
    let mut cinematic = mission.clone();
    if let Some(d) = &mut cinematic.dialogue {
        d.cinematic = true;
    }
    for _ in 0..60 {
        hud.show(
            &mut rig.game.host,
            &rig.gpu,
            &data,
            Some(&cinematic),
            0.016,
            view_h,
        );
        rig.game.prepare_ui(&rig.gpu, &mut rig.renderer, 1.0);
    }
    rig.shoot_as_is("mission-cinematic-sample")?;

    // A connection problem as a networked match shows it.
    rig.game.problem = Some((
        "Out of sync".into(),
        "The players' games disagreed at beat 4120, so the match cannot continue. This is a bug; the log has the details.".into(),
    ));
    rig.settle(0.1);
    rig.shoot("match-desync")?;
    rig.game.problem = None;

    rig.game.apply(Intent::Show(Overlay::GameMenu));
    rig.game.menu_page = MenuPage::Main;
    rig.settle(0.2);
    rig.shoot("game-menu")?;
    rig.click("gm.save");
    super::widgets::text(
        &mut rig.game.host.ui,
        "game-menu/save-page/name",
        "Before the river crossing",
    );
    rig.settle(0.1);
    rig.shoot("game-menu-save")?;
    rig.game.save_game();
    rig.game.save_game_named("Echo Isles opening");
    rig.settle(0.1);
    rig.shoot("game-menu-saved")?;
    rig.game.open_saves();
    rig.settle(0.1);
    rig.shoot("saves")?;
    // Loading the save continues the match with its camera and control groups.
    let saved = rig
        .game
        .saves_list
        .iter()
        .find(|m| m.name == "Echo Isles opening")
        .map(|m| m.id.clone())
        .context("the save is listed")?;
    rig.game.load_save(&saved);
    rig.until("the loaded match", Duration::from_secs(180), |g| {
        g.flow.in_match() && g.playing.is_some()
    })?;
    rig.settle(1.5);
    let groups = rig
        .game
        .playing
        .as_ref()
        .map(|p| p.game.play.groups.get(0).to_vec());
    anyhow::ensure!(
        groups.as_deref() == Some(mine.as_slice()),
        "control group 1 comes back with the save: {groups:?}"
    );
    rig.shoot("match-loaded")?;
    rig.game.apply(Intent::Show(Overlay::GameMenu));
    rig.game.menu_page = MenuPage::Main;
    rig.click("gm.exit");
    rig.settle(0.1);
    rig.shoot("game-menu-leave")?;
    rig.click("gm.page-back");
    rig.game.apply(Intent::Back);
    // The generic confirmation, as deleting a save asks it.
    rig.game.open_saves();
    let first = rig.game.saves_list.first().cloned();
    if let Some(meta) = first {
        rig.game.ask(super::Dialog {
            ask: super::Ask::DeleteSave(meta.id.clone()),
            title: "Delete this save?".into(),
            body: format!("“{}” is removed for good.", meta.name),
            confirm: "Delete".into(),
            danger: true,
            second: None,
        });
        rig.settle(0.1);
        rig.shoot("confirm-delete")?;
        rig.game.dialog = None;
        rig.game.apply(Intent::Back);
    }
    rig.game.apply(Intent::Back);

    // The result: a surrender, then a decided match.
    rig.game.decided(None, true);
    rig.settle(0.3);
    rig.shoot("result-defeat")?;
    let outcome = canopy_sim::state::Outcome {
        winner: Some(canopy_content::placement::Owner::Player(0)),
        defeated: vec![canopy_content::placement::Owner::Player(1)],
    };
    if let Some(playing) = &mut rig.game.playing {
        playing.outcome = Some(outcome.clone());
    }
    rig.game.decided(Some(outcome), false);
    rig.settle(0.2);
    rig.shoot("result-victory")?;
    rig.game.apply(Intent::LeaveMatch);
    rig.settle(0.2);
    rig.game.apply(Intent::Back);
    rig.settle(0.2);
    Ok(())
}
