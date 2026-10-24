//! `canopy shot --play [--input <script>]`: a match captured offscreen with the HUD,
//! from player 1's seat (or `--observe` for the spectator HUD), after a scripted input
//! sequence — deterministic captures of the in-match interface for review.
//!
//! The script is `;`-separated steps, positions in pixels of the capture:
//!
//! | Step | Does |
//! |---|---|
//! | `select:<text>` | selects the own entities whose definition id contains `<text>` |
//! | `focus:<text>` | selects the first such entity |
//! | `cmd:<id>` | presses the command `<id>` of the card (`build:<definition>`, `attack`) |
//! | `key:<name>` | presses a key (`R`, `Shift+R`, `Ctrl+1`, `F1`) |
//! | `move:x,y` · `click:x,y` · `rclick:x,y` · `drag:x0,y0,x1,y1` | the pointer |
//! | `world:x,z[,h]` · `rworld:x,z[,h]` · `lworld:x,z[,h]` | moves the pointer over a ground point (cells), or `h` above it, and right- or left-clicks it |
//! | `ping:x,z` | pings a ground point (cells) for the seat's team |
//! | `fund:<item>,<amount>` | sets the seat's stock of `<item>` |
//! | `fall:<text>` | the own hero whose definition id contains `<text>` falls (dies) |
//! | `revive:<text>` | selects the first finished own building that revives and revives that fallen hero |
//! | `look:x,z` · `look-selection` | the camera |
//! | `ticks:n` | simulates `n` ticks (commands sent so far apply) |
//! | `chat:<text>` | says a line |
//! | `network[:off]` | shows the power network on the ground, as the debug overlay does (D66) |
//! | `destroy:<text>` | destroys the own buildings whose definition id contains `<text>` |
//!
//! [`Rig`] is the match, the renderer and the interface kept together, so a capture can
//! step on and draw again (`play::preview` draws a whole mission scene with it).

use super::{
    Play,
    hud::Hud,
    input::{Button, Input, Mods},
    know::Knowledge,
    overlay,
    picking::{Body, Screen},
};
use crate::{
    live::LiveScene,
    options::Options,
    paths::Paths,
    sim::{self, Seat},
    view,
    world::{self, MatchSpec},
};
use anyhow::{Context, Result, bail};
use canopy_content::Content;
use canopy_render::{
    DeviceOptions, Gpu, Renderer, RtsCamera,
    capture::{CAPTURE_FORMAT, Capture},
};
use canopy_scene::live::Presentation;
use canopy_sim::{catalog::Catalog, view::View};
use canopy_ui::{FontSources, Ui, UiConfig};
use glam::{Vec2, Vec3};
use std::{path::Path, sync::Arc, time::Duration};

fn pair(text: &str) -> Result<Vec<f32>> {
    text.split(',')
        .map(|v| v.trim().parse::<f32>().map_err(anyhow::Error::from))
        .collect()
}

pub fn run(
    options: &Options,
    paths: &Paths,
    out: &Path,
    size: [u32; 2],
    observe: bool,
    script: &str,
) -> Result<()> {
    let map = options.map.as_deref().context("shot needs --map")?;
    let mut rig = Rig::new(options, paths, map, size, observe, None)?;
    while rig.game.world.tick() < options.tick {
        rig.step()?;
    }
    // Autoplay's camera follows its action, as in the game.
    if options.camera.is_none()
        && let Some([x, z]) = rig.autoplay_live().and_then(|a| a.focus)
    {
        rig.look_at(x, z);
    }
    rig.input(script)?;
    let pixels = rig.render(true)?;
    if std::env::var_os("CANOPY_SHOT_HUD").is_some()
        && let Some(texture) = rig.renderer.minimap_texture()
    {
        let image = Capture::new(&rig.gpu.device, 256, 256);
        let mut encoder = rig.gpu.device.create_command_encoder(&Default::default());
        image.copy_from(&mut encoder, texture);
        rig.gpu.queue.submit([encoder.finish()]);
        let rgba = image.read(&rig.gpu.device)?;
        image::save_buffer(
            out.with_extension("minimap.png"),
            &rgba,
            256,
            256,
            image::ExtendedColorType::Rgba8,
        )?;
    }
    rig.save(out, &pixels)?;
    println!(
        "{}: {map} tick {} · selected {:?} · sent {:?}",
        out.display(),
        rig.game.world.tick(),
        rig.play.selection.ids(),
        rig.play.sent.iter().map(|(_, a)| a).collect::<Vec<_>>()
    );
    Ok(())
}

/// A match with the player's interface, drawn offscreen on this thread.
pub struct Rig {
    pub gpu: Gpu,
    pub loaded: world::Loaded,
    pub game: world::Match,
    pub size: [u32; 2],
    me: Option<u8>,
    seat: Seat,
    /// What the local player is shown when it is not their own seat's view.
    shown: Option<Seat>,
    observe: bool,
    /// Animation time of the picture, seconds.
    time: f32,
    content: Arc<Content>,
    catalog: Arc<Catalog>,
    newest: Presentation,
    previous: Option<Presentation>,
    pub view: Arc<View>,
    renderer: Renderer,
    live: LiveScene,
    camera: RtsCamera,
    play: Play,
    /// The interface's clock.
    now: Duration,
    screen: Screen,
    ui: Ui,
    hud: Hud,
    /// `--autoplay`: the mission's plan plays player 1 as the match steps.
    autoplay: Option<canopy_sim::autoplay::Autoplay>,
}

impl Rig {
    /// Loads `map` and sets up the renderer and the interface: player 1's seat, or the
    /// spectator's (`observe`). `exploration` overrides what the map lets the players
    /// know at the start.
    pub fn new(
        options: &Options,
        paths: &Paths,
        map: &str,
        size: [u32; 2],
        observe: bool,
        exploration: Option<canopy_content::map::Exploration>,
    ) -> Result<Self> {
        let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
        let me = (!observe).then_some(0u8);
        let (loaded, mut game) = world::load(
            paths,
            &MatchSpec {
                map,
                hour: options.hour,
                players: options.players,
                crowd: None,
                gallery: false,
                only: None,
                colors: options.colors.clone(),
                races: options.races.clone(),
                biome: options.biome.as_deref(),
                weather: None,
                human: me,
                content: None,
                encounter: None,
                exploration,
                autoplay: options.autoplay,
            },
        )?;
        let autoplay = match &mut game.driver {
            sim::Driver::Live { autoplay, .. } => autoplay.take(),
            sim::Driver::Replay(_) => None,
        }
        .map(|spec| canopy_sim::autoplay::Autoplay::new(&spec.source, spec.map))
        .transpose()
        .map_err(|e| anyhow::anyhow!("autoplay plan: {e}"))?;
        let seat = me.map_or(Seat::Observer, Seat::Player);
        // Sandbox maps show revealed to a local player, as in the game (`run.rs`).
        let shown = (loaded.sandbox && me.is_some()).then_some(Seat::Observer);
        let content = game.world.catalog().content.clone();
        let catalog = Arc::new(Catalog::new(content.clone()));
        let commands = game.session.commands();
        let (mut newest, view) = sim::present(&game.world, &mut game.presenter, seat);
        if let Some(shown) = shown {
            newest = sim::present(&game.world, &mut game.presenter, shown).0;
        }

        let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, size, loaded.environment.clone());
        renderer.set_quality(&gpu, options.quality);
        renderer.load_world(&gpu, &loaded.pack, &loaded.scene.world)?;
        let mut live = LiveScene::default();
        live.attach_effects(
            &mut renderer,
            &gpu,
            &loaded.pack,
            game.presenter.effects.effect_table().clone(),
        );
        live.attach_world_ui(
            &paths.data,
            &loaded.pack,
            &content,
            loaded.scene.colors.clone(),
        );
        renderer.bake_minimap(
            &gpu,
            loaded.scene.size,
            loaded.scene.minimap,
            super::run::MINIMAP_PIXELS,
        );
        renderer.set_portrait(&gpu, None, super::run::PORTRAIT_PIXELS);
        let mut camera = view::initial(options.camera.and_then(|c| c.pose()), &loaded.scene);
        if me.is_some() {
            camera.zoom_range = canopy_render::camera::game_zoom();
            if options.camera.is_none()
                && let Some(&[x, z]) = loaded.scene.starts.first()
            {
                camera.look_at(x, z);
            }
        }
        let mut play = Play::new(
            knowledge(&content, &catalog, &view, me),
            game.world.terrain().clone(),
            Box::new(commands),
        );
        play.set_trees(Arc::new(super::picking::Trees::of_scene(
            &loaded.pack,
            &loaded.scene.world.statics,
            &loaded.scene.tree_slots,
        )));

        // The HUD.
        let mut config = UiConfig::load(&paths.data.join("config/ui"))?;
        crate::text::localize(&mut config, &paths.data);
        let mut ui = Ui::new(
            config,
            FontSources {
                bundled: Vec::new(),
                system: true,
            },
        )?;
        let mut hud = Hud::new();
        hud.configure(&game.world.catalog().content, game.world.setup(), me);
        hud.minimap_area = loaded.scene.minimap;
        hud.palette = loaded
            .scene
            .colors
            .iter()
            .map(|c| c.map(|v| (super::run::srgb(v) * 255.0).round() as u8))
            .collect();
        hud.attach(&mut ui);
        // The minimap and the live portrait, as the window's HUD shows them.
        for (name, id, view) in [
            ("@minimap", 1, renderer.minimap_view().cloned()),
            ("@portrait", 2, renderer.portrait_view().cloned()),
        ] {
            let Some(view) = view else { continue };
            let id = canopy_ui::TextureId(id);
            renderer.ui_mut(&gpu).set_texture(&gpu, id, &view);
            let mut desc = canopy_ui::ImageDesc::new(id, 256.0, 256.0);
            desc.premultiplied = true;
            desc.srgb_view = true;
            ui.register_image(name, desc);
            match name {
                "@minimap" => hud.minimap = true,
                _ => hud.live_portrait = true,
            }
        }
        let layer = renderer.ui_mut(&gpu);
        for (name, asset) in ui
            .theme_images()
            .map(|(n, a)| (n.to_owned(), a.to_owned()))
            .collect::<Vec<_>>()
        {
            if let Ok(desc) = layer.load_pack_image(&gpu, &loaded.pack, &asset) {
                ui.register_image(&name, desc);
            }
        }
        ui.set_viewport(size[0], size[1], 1.0);

        let aspect = size[0] as f32 / size[1] as f32;
        let screen = Screen {
            camera: view::frame(&mut camera, aspect, &loaded.scene),
            size: [size[0] as f32, size[1] as f32],
            scale: 1.0,
        };
        let mut rig = Self {
            gpu,
            loaded,
            game,
            size,
            me,
            seat,
            shown,
            observe,
            time: options.time,
            content,
            catalog,
            newest,
            previous: None,
            view,
            renderer,
            live,
            camera,
            play,
            now: Duration::from_secs(5),
            screen,
            ui,
            hud,
            autoplay,
        };
        rig.refresh();
        Ok(rig)
    }

    /// Simulates one tick (commands sent so far apply).
    pub fn step(&mut self) -> Result<()> {
        let game = &mut self.game;
        let advanced = sim::advance(
            &mut game.world,
            &mut game.session,
            &mut game.presenter,
            self.seat,
            self.shown,
        )
        .with_context(|| format!("the local session stopped at tick {}", game.world.tick()))?;
        self.previous = Some(std::mem::replace(&mut self.newest, advanced.presentation));
        self.view = advanced.view;
        if let Some(plan) = &mut self.autoplay {
            for action in plan.tick(&mut game.world) {
                game.session.send(action);
            }
            for line in plan.drain_lines() {
                println!("{line}");
            }
        }
        Ok(())
    }

    /// Autoplay as the HUD shows it (at its starting speed; 1× while a cinematic plays),
    /// and where its camera looks.
    fn autoplay_live(&self) -> Option<sim::AutoplayLive> {
        let plan = self.autoplay.as_ref()?;
        let world = &self.game.world;
        let cinematic = canopy_sim::mission::blocks_commands(world.state()).is_some();
        Some(sim::AutoplayLive {
            on: true,
            speed: sim::AUTOPLAY_SPEED,
            running: if cinematic { 1.0 } else { sim::AUTOPLAY_SPEED },
            label: plan.label().to_string(),
            focus: plan.focus(world).map(|(x, z)| [x, z]),
        })
    }

    /// Points the game's camera at a ground point (cells), as the player would.
    pub fn look_at(&mut self, x: f32, z: f32) {
        self.camera.look_at(x, z);
    }

    /// Moves the interface's clock on, as the window's frames would.
    pub fn advance_clock(&mut self, by: Duration) {
        self.now += by;
    }

    fn know(&self) -> Knowledge {
        knowledge(&self.content, &self.catalog, &self.view, self.me)
    }

    /// Shows the newest tick and brings the interface up to date with it.
    fn refresh(&mut self) -> Screen {
        self.live.show(
            &mut self.renderer,
            &self.gpu,
            &self.loaded.pack,
            self.previous.as_ref(),
            &self.newest,
            1.0,
        );
        let aspect = self.size[0] as f32 / self.size[1] as f32;
        let camera_view = view::frame(&mut self.camera, aspect, &self.loaded.scene);
        let screen = Screen {
            camera: camera_view,
            size: [self.size[0] as f32, self.size[1] as f32],
            scale: 1.0,
        };
        let k = self.know();
        let bodies: Vec<Body> = self
            .live
            .bodies()
            .iter()
            .map(|&(id, [radius, height], foot)| Body {
                id,
                foot,
                height,
                radius: (radius - 0.32).max(0.2),
                capsule: k.entity(id).is_some_and(|e| k.picked_by_body(e)),
            })
            .collect();
        self.play
            .update(k, screen, bodies, self.loaded.scene.terrain(), self.now);
        self.screen = screen;
        screen
    }

    /// Runs an input script (the module's table).
    pub fn input(&mut self, script: &str) -> Result<()> {
        let mut mods = Mods::default();
        for raw in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let (verb, arg) = raw.split_once(':').unwrap_or((raw, ""));
            self.input_step(verb, arg, &mut mods)?;
            self.now += Duration::from_millis(120);
            self.refresh();
            for e in self.play.take_events() {
                if let super::Event::LookAt([x, z]) = e {
                    self.camera.look_at(x, z);
                    self.refresh();
                }
            }
        }
        Ok(())
    }

    fn input_step(&mut self, verb: &str, arg: &str, mods: &mut Mods) -> Result<()> {
        let k = self.play.knowledge().clone();
        let terrain = || self.loaded.scene.terrain();
        match verb {
            "select" | "focus" => {
                let mut ids: Vec<u32> = k
                    .view
                    .entities
                    .iter()
                    .filter(|e| k.own(e) && k.alive(e) && k.def(e).id.as_str().contains(arg))
                    .map(|e| e.id().0)
                    .collect();
                if verb == "focus" {
                    ids.truncate(1);
                }
                self.play.select(ids, None);
            }
            "cmd" => self.play.activate_id(arg, mods.shift),
            "key" => {
                let mut m = Mods::default();
                let mut key = arg;
                while let Some((modifier, rest)) = key.split_once('+') {
                    match modifier {
                        "Shift" => m.shift = true,
                        "Ctrl" => m.ctrl = true,
                        "Alt" => m.alt = true,
                        other => bail!("unknown modifier {other}"),
                    }
                    key = rest;
                }
                let name = super::input::KEY_NAMES
                    .iter()
                    .find(|n| **n == key)
                    .with_context(|| format!("unknown key {key}"))?;
                let terrain = terrain();
                self.play.input(&Input::Modifiers(m), terrain);
                for pressed in [true, false] {
                    self.play.input(
                        &Input::Key {
                            key: name,
                            pressed,
                            repeat: false,
                        },
                        terrain,
                    );
                }
                self.play.input(&Input::Modifiers(*mods), terrain);
            }
            "shift" => {
                mods.shift = arg != "off";
                self.play.input(&Input::Modifiers(*mods), terrain());
            }
            "move" | "click" | "rclick" => {
                let p = pair(arg)?;
                let terrain = terrain();
                self.play
                    .input(&Input::PointerMoved(Vec2::new(p[0], p[1])), terrain);
                if verb != "move" {
                    let button = if verb == "click" {
                        Button::Left
                    } else {
                        Button::Right
                    };
                    for pressed in [true, false] {
                        self.play.input(&Input::Button { button, pressed }, terrain);
                    }
                }
            }
            "world" | "rworld" | "lworld" => {
                let p = pair(arg)?;
                let terrain = terrain();
                let y = canopy_render::camera::Ground::height(terrain, p[0], p[1])
                    + p.get(2).copied().unwrap_or(0.0);
                let (at, _) = self
                    .screen
                    .project(Vec3::new(p[0], y, p[1]))
                    .context("off screen")?;
                self.play.input(&Input::PointerMoved(at), terrain);
                if verb != "world" {
                    let button = if verb == "rworld" {
                        Button::Right
                    } else {
                        Button::Left
                    };
                    for pressed in [true, false] {
                        self.play.input(&Input::Button { button, pressed }, terrain);
                    }
                }
            }
            "drag" | "hold-drag" => {
                let p = pair(arg)?;
                let terrain = terrain();
                self.play
                    .input(&Input::PointerMoved(Vec2::new(p[0], p[1])), terrain);
                self.play.input(
                    &Input::Button {
                        button: Button::Left,
                        pressed: true,
                    },
                    terrain,
                );
                self.play
                    .input(&Input::PointerMoved(Vec2::new(p[2], p[3])), terrain);
                if verb == "drag" {
                    self.play.input(
                        &Input::Button {
                            button: Button::Left,
                            pressed: false,
                        },
                        terrain,
                    );
                }
            }
            "look" => {
                let p = pair(arg)?;
                self.camera.look_at(p[0], p[1]);
            }
            "look-selection" => {
                let points: Vec<[f32; 2]> = self
                    .play
                    .selection
                    .ids()
                    .iter()
                    .filter_map(|&id| k.entity(id))
                    .map(|e| k.position(e))
                    .collect();
                if let Some(p) = points.first() {
                    self.camera.look_at(p[0], p[1]);
                }
            }
            "ticks" => {
                let n: u32 = arg.parse().context("ticks:n")?;
                for _ in 0..n {
                    self.step()?;
                }
            }
            "chat" => self.play.say(arg),
            "network" => self.play.show_network = arg != "off",
            "destroy" => {
                let targets: Vec<u32> = k
                    .view
                    .entities
                    .iter()
                    .filter(|e| k.own(e) && k.alive(e) && k.is_building(e))
                    .filter(|e| k.def(e).id.as_str().contains(arg))
                    .map(|e| e.id().0)
                    .collect();
                for id in targets {
                    let target = canopy_sim::entity::EntityId(id);
                    let state = self.game.world.state_mut();
                    state.combat.scripted.push(canopy_sim::combat::ScriptedHit {
                        target,
                        source: None,
                        amount: 1,
                    });
                    if let Some(e) = state.entities.get_mut(target) {
                        e.hp = Some(1);
                    }
                }
                self.step()?;
            }
            "ping" => {
                let p = pair(arg)?;
                self.play.ping(p[0], p[1]);
            }
            "fund" => {
                let (item, amount) = arg.split_once(',').context("fund:<item>,<amount>")?;
                let id = canopy_content::Id::new(item).map_err(anyhow::Error::msg)?;
                let amount: u32 = amount.parse().context("an amount")?;
                let owner = k.owner().context("a seat")?;
                self.game
                    .world
                    .state_mut()
                    .wallets
                    .entry(owner)
                    .or_default()
                    .insert(id, amount);
                self.step()?;
            }
            "xp" => {
                // Experience for the player's heroes whose definition id contains the
                // fragment (a level-up to look at).
                let (fragment, amount) = arg.split_once(',').context("xp:<hero>,<amount>")?;
                let amount: u32 = amount.parse().context("an amount")?;
                let heroes: Vec<u32> = k
                    .view
                    .entities
                    .iter()
                    .filter(|e| k.own(e) && k.alive(e) && k.def(e).is_hero())
                    .filter(|e| k.def(e).id.as_str().contains(fragment))
                    .map(|e| e.id().0)
                    .collect();
                for id in heroes {
                    let state = self.game.world.state_mut();
                    if let Some(p) = state
                        .entities
                        .get_mut(canopy_sim::entity::EntityId(id))
                        .and_then(|e| e.progression.as_mut())
                    {
                        p.experience += amount;
                    }
                }
                self.step()?;
            }
            "fall" => {
                let heroes: Vec<u32> = k
                    .view
                    .entities
                    .iter()
                    .filter(|e| k.own(e) && k.alive(e) && k.def(e).is_hero())
                    .filter(|e| k.def(e).id.as_str().contains(arg))
                    .map(|e| e.id().0)
                    .collect();
                for id in heroes {
                    let state = self.game.world.state_mut();
                    if let Some(e) = state.entities.get_mut(canopy_sim::entity::EntityId(id)) {
                        canopy_sim::heroes::retain(e);
                    }
                }
                self.step()?;
            }
            "revive" => {
                let hero = k
                    .view
                    .battle
                    .fallen
                    .iter()
                    .map(|id| id.0)
                    .find(|&id| {
                        k.entity(id)
                            .is_some_and(|e| k.def(e).id.as_str().contains(arg))
                    })
                    .context("a fallen hero")?;
                let building = k
                    .view
                    .entities
                    .iter()
                    .find(|e| {
                        k.own(e)
                            && e.public.construction.is_none()
                            && k.def(e).behaviors.revival.is_some()
                    })
                    .context("a finished building that revives")?
                    .id()
                    .0;
                self.play.select(vec![building], None);
                self.play.activate_id(&format!("revive:{hero}"), false);
            }
            other => bail!("unknown step {other}"),
        }
        Ok(())
    }

    /// Draws the newest tick as the window would: the camera a scene or the unit camera
    /// holds, the world, the HUD (or the mission's letterbox and speaker), and returns
    /// the picture's RGBA pixels. `timing` measures and prints what the HUD costs.
    pub fn render(&mut self, timing: bool) -> Result<Vec<u8>> {
        let size = self.size;
        let mut screen = self.refresh();
        self.live.hidden_entity = None;
        let terrain = self.loaded.scene.terrain();
        // A close camera (unit shot, unit camera) keeps the canopies whole.
        let mut close_camera = false;
        // A saved-camera shot (`mission.camera_to`) frames the picture at this tick, blends
        // included, while cinematic mode lasts or a blend runs.
        let tick = self.game.world.state().tick as f32;
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let cinema_on = self
            .view
            .mission
            .as_ref()
            .is_some_and(|m| m.cinema.on && m.error.is_none());
        let targets = sim::scene_targets(&self.game.world, &self.view);
        let framed = targets
            .as_ref()
            .and_then(|t| t.frames)
            .filter(|f| f.returned(tick).is_none() && (cinema_on || f.progress(tick) < 1.0));
        if let Some(frames) = &framed {
            let stage = super::presets::Live {
                live: &self.live,
                actors: self.renderer.actors(),
                obstacles: &self.loaded.obstacles,
                ground: terrain,
                size: self.loaded.scene.size as f32,
            };
            let saved = frames.at(tick, &self.camera, aspect, &mut |p| {
                super::presets::place(p, &stage, aspect, None)
            });
            screen.camera = saved.view(aspect, terrain);
            close_camera = true;
        } else if let Some([x, z]) = targets
            .as_ref()
            .and_then(|t| t.frames.as_ref())
            .and_then(|f| f.returned(tick))
            .flatten()
        {
            self.camera.look_at(x, z);
            screen.camera = view::frame(&mut self.camera, aspect, &self.loaded.scene);
        }
        // A mission cinematic looks as the window would once its blends end: a scene's unit
        // shot through its unit, else at the scene through the narrower lens.
        if let (None, Some(scene)) = (&framed, self.play.cinematic()) {
            let shot = self.play.scene_camera().cloned();
            let (live, content, catalog) = (&self.live, &self.content, &self.catalog);
            let subject = |t: &sim::SceneTarget| {
                super::follow::subject_of(live, content, catalog.get(t.definition), t.id)
            };
            let unit = shot
                .as_ref()
                .zip(targets.as_ref())
                .and_then(|(camera, targets)| {
                    let obstruction =
                        |o, d, far| live.camera_obstruction(&self.loaded.obstacles, o, d, far);
                    super::cinematic::pose(camera, targets, subject, terrain, &obstruction)
                });
            match unit {
                Some((key, pose)) => {
                    if key.mode == super::follow::CameraMode::FirstPerson {
                        self.live.hidden_entity = Some(key.entity);
                        self.live.show(
                            &mut self.renderer,
                            &self.gpu,
                            &self.loaded.pack,
                            self.previous.as_ref(),
                            &self.newest,
                            1.0,
                        );
                    }
                    screen.camera = super::follow::Follow::default().view(key, &pose, 10.0, aspect);
                    close_camera = true;
                }
                None => {
                    if let Some(scene) = scene {
                        let [x, z] =
                            super::cinematic::focus(scene, shot.as_ref(), targets.as_ref());
                        self.camera.look_at(x, z);
                    }
                    let lens = super::cinematic::Lens { blend: 1.0 };
                    screen.camera =
                        lens.apply(view::frame(&mut self.camera, aspect, &self.loaded.scene));
                }
            }
        }

        // A unit camera (J) looks from the focused unit, as the window would once its blend
        // ends; the first-person body is not drawn.
        let mode = self.play.camera_mode;
        if mode != super::follow::CameraMode::Rts && self.play.cinematic().is_none() {
            self.live.hidden_entity = (mode == super::follow::CameraMode::FirstPerson)
                .then(|| self.play.selection.focus())
                .flatten();
            screen = self.refresh();
            let terrain = self.loaded.scene.terrain();
            if let Some(pose) = self.play.selection.focus().and_then(|id| {
                let subject = super::follow::subject(&self.live, self.play.knowledge(), id)?;
                let obstruction = |o, d, far| {
                    self.live
                        .camera_obstruction(&self.loaded.obstacles, o, d, far)
                };
                super::follow::ground_pose(&subject, mode, terrain, &obstruction)
            }) {
                screen.camera = super::follow::Follow::default().view(
                    super::follow::Key::unit(mode, 0),
                    &pose,
                    super::follow::TRANSITION_SECONDS,
                    aspect,
                );
                close_camera = true;
            }
        }

        let terrain = self.loaded.scene.terrain();

        // How the simulation judges the ghost's site (it knows everything).
        if let (Some(ghost), super::Mode::Place { placement, .. }) =
            (self.play.ghost(), &self.play.mode)
        {
            let verdict = self.game.world.can_build(
                canopy_content::placement::Owner::Player(0),
                &placement.definition,
                &ghost.point,
                placement
                    .actors
                    .first()
                    .map(|&a| canopy_sim::entity::EntityId(a)),
                placement.rotation,
            );
            println!(
                "placement at {:?}: ghost {:?}, simulation {:?}",
                ghost.point, ghost.problem, verdict
            );
        }

        let now = self.now;
        if self.observe {
            self.hud.colonies = Some(Arc::new(sim::colonies(&self.game.world)));
        }
        let autoplay = self.autoplay_live();
        let (ui, hud) = (&mut self.ui, &mut self.hud);
        ui.show("hud", true);
        // Two passes: the first lists the images the HUD needs.
        for _ in 0..2 {
            hud.update(ui, &self.play);
            ui.frame(now.as_secs_f64());
            for name in hud.take_image_requests() {
                if let Ok(desc) = self.renderer.ui_mut(&self.gpu).load_pack_image(
                    &self.gpu,
                    &self.loaded.pack,
                    &name,
                ) {
                    ui.register_image(&name, desc);
                }
            }
        }
        if timing {
            // What binding and painting the HUD costs per frame (steady state).
            let started = std::time::Instant::now();
            for i in 0..100 {
                self.play.update(
                    self.play.knowledge().clone(),
                    screen,
                    Vec::new(),
                    terrain,
                    now + Duration::from_millis(i),
                );
                hud.update(ui, &self.play);
                ui.frame(now.as_secs_f64());
            }
            println!(
                "  hud: update + bind + paint {:.3} ms per frame",
                started.elapsed().as_secs_f64() * 10.0
            );
        }
        let view0 = &self.view;
        // A cinematic line's speaker talks in the portrait.
        let speaker = view0
            .mission
            .as_ref()
            .filter(|m| m.cinema.on)
            .and_then(|m| m.dialogue.as_ref())
            .filter(|d| d.until > view0.tick)
            .and_then(|d| d.actor)
            .and_then(|a| self.live.portrait(a.0, 12.0))
            .map(|s| canopy_render::PortraitSubject { close: true, ..s });
        let live_speaker = speaker.is_some();
        // The mission HUD over the HUD: letterbox, transmission, fade, objectives.
        let cinema = view0
            .mission
            .as_ref()
            .is_some_and(|m| m.cinema.on && m.error.is_none());
        if let Some(m) = view0.mission.as_ref() {
            ui.show("mission-hud", true);
            ui.frame(now.as_secs_f64());
            let icon = m
                .dialogue
                .as_ref()
                .and_then(|d| self.content.definition(d.portrait.as_str()))
                .and_then(|def| {
                    crate::screens::widgets::content_image(&self.content, def.icon.as_str())
                })
                .and_then(|name| {
                    let desc = self
                        .renderer
                        .ui_mut(&self.gpu)
                        .load_pack_image(&self.gpu, &self.loaded.pack, &name)
                        .ok()?;
                    ui.register_image(&name, desc);
                    Some(ui.image_key(&name))
                });
            let objectives = self
                .game
                .world
                .mission()
                .map(|r| r.script.objectives().to_vec())
                .unwrap_or_default();
            let mut mission_hud = crate::screens::match_ui::MissionHud::new(objectives);
            mission_hud.shown = view0.tick as f32;
            mission_hud.live_speaker = live_speaker;
            mission_hud.autoplay = autoplay;
            mission_hud.present(
                ui,
                icon,
                Some(m),
                view0.tick,
                view0.outcome.is_some(),
                10.0,
                size[1] as f32,
            );
        }
        hud.update(ui, &self.play);
        // Cinematic mode hides the HUD.
        if cinema {
            ui.show("hud", false);
        }
        // Atlas uploads are incremental and only this frame reaches the GPU.
        ui.reset_glyph_atlas();
        let mut list = ui.frame(now.as_secs_f64()).clone();
        let world_ui = self.play.world_ui();
        overlay::draw(&mut list, screen.scale, &world_ui);
        hud.draw_motes(&mut list, self.play.now(), screen.scale);
        self.live.world_ui_input = super::run::world_ui_input(&world_ui, screen.scale, self.me);
        self.live.eye = Some(screen.camera.eye);
        self.renderer.set_cutaway(
            (!close_camera)
                .then(|| self.live.cutaway_subjects())
                .into_iter()
                .flatten(),
        );
        self.live.draw_world_ui(
            &mut self.renderer,
            &self.gpu,
            &screen.camera,
            &self.loaded.scene,
            12.0,
        );
        let pings = self.play.minimap_pings(&self.loaded.scene.colors);
        self.live.draw_minimap(
            &mut self.renderer,
            &screen.camera,
            &self.loaded.scene,
            super::run::MINIMAP_PIXELS,
            &pings,
        );
        let subject = speaker.or_else(|| {
            world_ui
                .selected
                .first()
                .and_then(|&id| self.live.portrait(id, 12.0))
        });
        self.renderer
            .set_portrait(&self.gpu, subject, super::run::PORTRAIT_PIXELS);
        if let (Some(ghost), Some(def)) = (&world_ui.ghost, &world_ui.ghost_definition) {
            let mut instances = self.live.instances().to_vec();
            if let Some(i) = super::run::ghost_instance(
                &self.content,
                &self.newest,
                ghost,
                def,
                terrain,
                self.me,
                &self.loaded.scene.colors,
            ) {
                instances.push(i);
            }
            self.renderer.set_actors(&instances);
        }
        let capture = Capture::new(&self.gpu.device, size[0], size[1]);
        let layer = self.renderer.ui_mut(&self.gpu);
        layer.submit(&self.gpu, &list);
        layer.set_target(Some(capture.ui_view.clone()));
        let copy = |encoder: &mut wgpu::CommandEncoder| capture.copy(encoder);
        self.renderer.render(
            &self.gpu,
            &screen.camera,
            self.time,
            &capture.view,
            Some(&copy),
        );
        self.gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
        capture.read(&self.gpu.device)
    }

    /// Writes a picture [`Rig::render`] returned to `out` (a PNG).
    pub fn save(&self, out: &Path, pixels: &[u8]) -> Result<()> {
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        image::save_buffer(
            out,
            pixels,
            self.size[0],
            self.size[1],
            image::ExtendedColorType::Rgba8,
        )
        .with_context(|| out.display().to_string())
    }
}

/// What the local player knows from `view`.
fn knowledge(
    content: &Arc<Content>,
    catalog: &Arc<Catalog>,
    view: &Arc<View>,
    me: Option<u8>,
) -> Knowledge {
    let mut k = Knowledge::new(content.clone(), catalog.clone(), view.clone(), me);
    k.held = Arc::new(sim::held_units(view));
    k
}
