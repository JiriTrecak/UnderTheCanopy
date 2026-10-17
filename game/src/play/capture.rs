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
use canopy_render::{
    DeviceOptions, Gpu, Renderer,
    capture::{CAPTURE_FORMAT, Capture},
};
use canopy_sim::catalog::Catalog;
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
    let gpu = Gpu::headless(DeviceOptions::default())?.context("no GPU adapter")?;
    let map = options.map.as_deref().context("shot needs --map")?;
    let me = (!observe).then_some(0u8);
    let (loaded, mut game) = world::load(
        paths,
        &MatchSpec {
            map,
            hour: options.hour,
            players: options.players,
            crowd: None,
            gallery: false,
            colors: options.colors.clone(),
            biome: options.biome.as_deref(),
            human: me,
            content: None,
            encounter: None,
        },
    )?;
    let seat = me.map_or(Seat::Observer, Seat::Player);
    let content = game.world.catalog().content.clone();
    let catalog = Arc::new(Catalog::new(content.clone()));
    let commands = game.session.commands();
    let (mut newest, mut view0) = sim::present(&game.world, &mut game.presenter, seat);
    let mut previous = None;
    let step = |game: &mut world::Match,
                newest: &mut canopy_scene::live::Presentation,
                view: &mut Arc<canopy_sim::view::View>,
                previous: &mut Option<canopy_scene::live::Presentation>|
     -> Result<()> {
        let advanced = sim::advance(
            &mut game.world,
            &mut game.session,
            &mut game.presenter,
            seat,
            None,
        )
        .with_context(|| format!("the local session stopped at tick {}", game.world.tick()))?;
        *previous = Some(std::mem::replace(newest, advanced.presentation));
        *view = advanced.view;
        Ok(())
    };
    while game.world.tick() < options.tick {
        step(&mut game, &mut newest, &mut view0, &mut previous)?;
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
    let aspect = size[0] as f32 / size[1] as f32;
    let terrain = loaded.scene.terrain();
    let know = |view: &Arc<canopy_sim::view::View>| {
        let mut k = Knowledge::new(content.clone(), catalog.clone(), view.clone(), me);
        k.held = Arc::new(sim::held_units(view));
        k
    };
    let mut play = Play::new(
        know(&view0),
        game.world.terrain().clone(),
        Box::new(commands),
    );
    play.set_trees(Arc::new(super::picking::Trees::of_scene(
        &loaded.pack,
        &loaded.scene.world.statics,
        &loaded.scene.tree_slots,
    )));
    let mut now = Duration::from_secs(5);
    let refresh = |play: &mut Play,
                   live: &mut LiveScene,
                   renderer: &mut Renderer,
                   camera: &mut canopy_render::RtsCamera,
                   newest: &canopy_scene::live::Presentation,
                   previous: Option<&canopy_scene::live::Presentation>,
                   view: &Arc<canopy_sim::view::View>,
                   now: Duration|
     -> Screen {
        live.show(renderer, &gpu, &loaded.pack, previous, newest, 1.0);
        let camera_view = view::frame(camera, aspect, &loaded.scene);
        let screen = Screen {
            camera: camera_view,
            size: [size[0] as f32, size[1] as f32],
            scale: 1.0,
        };
        let k = know(view);
        let bodies: Vec<Body> = live
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
        play.update(k, screen, bodies, terrain, now);
        screen
    };
    let mut screen = refresh(
        &mut play,
        &mut live,
        &mut renderer,
        &mut camera,
        &newest,
        previous.as_ref(),
        &view0,
        now,
    );
    let mut mods = Mods::default();
    for raw in script.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (verb, arg) = raw.split_once(':').unwrap_or((raw, ""));
        let k = play.knowledge().clone();
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
                play.select(ids, None);
            }
            "cmd" => play.activate_id(arg, mods.shift),
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
                play.input(&Input::Modifiers(m), terrain);
                play.input(
                    &Input::Key {
                        key: name,
                        pressed: true,
                        repeat: false,
                    },
                    terrain,
                );
                play.input(
                    &Input::Key {
                        key: name,
                        pressed: false,
                        repeat: false,
                    },
                    terrain,
                );
                play.input(&Input::Modifiers(mods), terrain);
            }
            "shift" => {
                mods.shift = arg != "off";
                play.input(&Input::Modifiers(mods), terrain);
            }
            "move" | "click" | "rclick" => {
                let p = pair(arg)?;
                let at = Vec2::new(p[0], p[1]);
                play.input(&Input::PointerMoved(at), terrain);
                if verb != "move" {
                    let button = if verb == "click" {
                        Button::Left
                    } else {
                        Button::Right
                    };
                    play.input(
                        &Input::Button {
                            button,
                            pressed: true,
                        },
                        terrain,
                    );
                    play.input(
                        &Input::Button {
                            button,
                            pressed: false,
                        },
                        terrain,
                    );
                }
            }
            "world" | "rworld" | "lworld" => {
                let p = pair(arg)?;
                let y = canopy_render::camera::Ground::height(terrain, p[0], p[1])
                    + p.get(2).copied().unwrap_or(0.0);
                let (at, _) = screen
                    .project(Vec3::new(p[0], y, p[1]))
                    .context("off screen")?;
                play.input(&Input::PointerMoved(at), terrain);
                if verb != "world" {
                    let button = if verb == "rworld" {
                        Button::Right
                    } else {
                        Button::Left
                    };
                    for pressed in [true, false] {
                        play.input(&Input::Button { button, pressed }, terrain);
                    }
                }
            }
            "drag" => {
                let p = pair(arg)?;
                play.input(&Input::PointerMoved(Vec2::new(p[0], p[1])), terrain);
                play.input(
                    &Input::Button {
                        button: Button::Left,
                        pressed: true,
                    },
                    terrain,
                );
                play.input(&Input::PointerMoved(Vec2::new(p[2], p[3])), terrain);
                if verb == "drag" {
                    play.input(
                        &Input::Button {
                            button: Button::Left,
                            pressed: false,
                        },
                        terrain,
                    );
                }
            }
            "hold-drag" => {
                let p = pair(arg)?;
                play.input(&Input::PointerMoved(Vec2::new(p[0], p[1])), terrain);
                play.input(
                    &Input::Button {
                        button: Button::Left,
                        pressed: true,
                    },
                    terrain,
                );
                play.input(&Input::PointerMoved(Vec2::new(p[2], p[3])), terrain);
            }
            "look" => {
                let p = pair(arg)?;
                camera.look_at(p[0], p[1]);
            }
            "look-selection" => {
                let points: Vec<[f32; 2]> = play
                    .selection
                    .ids()
                    .iter()
                    .filter_map(|&id| k.entity(id))
                    .map(|e| k.position(e))
                    .collect();
                if let Some(p) = points.first() {
                    camera.look_at(p[0], p[1]);
                }
            }
            "ticks" => {
                let n: u32 = arg.parse().context("ticks:n")?;
                for _ in 0..n {
                    step(&mut game, &mut newest, &mut view0, &mut previous)?;
                }
            }
            "chat" => play.say(arg),
            "ping" => {
                let p = pair(arg)?;
                play.ping(p[0], p[1]);
            }
            "fund" => {
                let (item, amount) = arg.split_once(',').context("fund:<item>,<amount>")?;
                let id = canopy_content::Id::new(item).map_err(anyhow::Error::msg)?;
                let amount: u32 = amount.parse().context("an amount")?;
                let owner = k.owner().context("a seat")?;
                game.world
                    .state_mut()
                    .wallets
                    .entry(owner)
                    .or_default()
                    .insert(id, amount);
                step(&mut game, &mut newest, &mut view0, &mut previous)?;
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
                    let state = game.world.state_mut();
                    if let Some(e) = state.entities.get_mut(canopy_sim::entity::EntityId(id)) {
                        canopy_sim::heroes::retain(e);
                    }
                }
                step(&mut game, &mut newest, &mut view0, &mut previous)?;
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
                play.select(vec![building], None);
                play.activate_id(&format!("revive:{hero}"), false);
            }
            other => bail!("unknown step {other}"),
        }
        now += Duration::from_millis(120);
        screen = refresh(
            &mut play,
            &mut live,
            &mut renderer,
            &mut camera,
            &newest,
            previous.as_ref(),
            &view0,
            now,
        );
        for e in play.take_events() {
            if let super::Event::LookAt([x, z]) = e {
                camera.look_at(x, z);
                screen = refresh(
                    &mut play,
                    &mut live,
                    &mut renderer,
                    &mut camera,
                    &newest,
                    previous.as_ref(),
                    &view0,
                    now,
                );
            }
        }
    }

    // A close camera (unit shot, unit camera) keeps the canopies whole.
    let mut close_camera = false;
    // A mission cinematic looks as the window would once its blends end: a scene's unit
    // shot through its unit, else at the scene through the narrower lens.
    if let Some(scene) = play.cinematic() {
        let targets = sim::scene_targets(&game.world, &view0);
        let shot = play.scene_camera().cloned();
        let subject = |t: &sim::SceneTarget| {
            super::follow::subject_of(&live, &content, catalog.get(t.definition), t.id)
        };
        let unit = shot
            .as_ref()
            .zip(targets.as_ref())
            .and_then(|(camera, targets)| {
                let obstruction = |o, d, far| live.camera_obstruction(&loaded.obstacles, o, d, far);
                super::cinematic::pose(camera, targets, subject, terrain, &obstruction)
            });
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        match unit {
            Some((key, pose)) => {
                if key.mode == super::follow::CameraMode::FirstPerson {
                    live.hidden_entity = Some(key.entity);
                    live.show(
                        &mut renderer,
                        &gpu,
                        &loaded.pack,
                        previous.as_ref(),
                        &newest,
                        1.0,
                    );
                }
                screen.camera = super::follow::Follow::default().view(key, &pose, 10.0, aspect);
                close_camera = true;
            }
            None => {
                if let Some(scene) = scene {
                    let [x, z] = super::cinematic::focus(scene, shot.as_ref(), targets.as_ref());
                    camera.look_at(x, z);
                }
                let lens = super::cinematic::Lens { blend: 1.0 };
                screen.camera = lens.apply(view::frame(&mut camera, aspect, &loaded.scene));
            }
        }
    }

    // A unit camera (J) looks from the focused unit, as the window would once its blend
    // ends; the first-person body is not drawn.
    let mode = play.camera_mode;
    if mode != super::follow::CameraMode::Rts && play.cinematic().is_none() {
        live.hidden_entity = (mode == super::follow::CameraMode::FirstPerson)
            .then(|| play.selection.focus())
            .flatten();
        screen = refresh(
            &mut play,
            &mut live,
            &mut renderer,
            &mut camera,
            &newest,
            previous.as_ref(),
            &view0,
            now,
        );
        if let Some(pose) = play.selection.focus().and_then(|id| {
            let subject = super::follow::subject(&live, play.knowledge(), id)?;
            let obstruction = |o, d, far| live.camera_obstruction(&loaded.obstacles, o, d, far);
            super::follow::ground_pose(&subject, mode, terrain, &obstruction)
        }) {
            let aspect = size[0] as f32 / size[1].max(1) as f32;
            screen.camera = super::follow::Follow::default().view(
                super::follow::Key::unit(mode, 0),
                &pose,
                super::follow::TRANSITION_SECONDS,
                aspect,
            );
            close_camera = true;
        }
    }

    // How the simulation judges the ghost's site (it knows everything).
    if let (Some(ghost), super::Mode::Place { placement, .. }) = (play.ghost(), &play.mode) {
        let verdict = game.world.can_build(
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

    // The HUD.
    let mut ui = Ui::new(
        UiConfig::load(&paths.data.join("config/ui"))?,
        FontSources {
            bundled: Vec::new(),
            system: true,
        },
    )?;
    let mut hud = Hud::new();
    hud.minimap_area = loaded.scene.minimap;
    hud.palette = loaded
        .scene
        .colors
        .iter()
        .map(|c| c.map(|v| (super::run::srgb(v) * 255.0).round() as u8))
        .collect();
    if observe {
        hud.colonies = Some(Arc::new(sim::colonies(&game.world)));
    }
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
    // Two passes: the first lists the images the HUD needs.
    for _ in 0..2 {
        hud.update(&mut ui, &play);
        ui.frame(now.as_secs_f64());
        for name in hud.take_image_requests() {
            if let Ok(desc) = renderer
                .ui_mut(&gpu)
                .load_pack_image(&gpu, &loaded.pack, &name)
            {
                ui.register_image(&name, desc);
            }
        }
    }
    // What binding and painting the HUD costs per frame (steady state).
    let started = std::time::Instant::now();
    for i in 0..100 {
        play.update(
            play.knowledge().clone(),
            screen,
            Vec::new(),
            terrain,
            now + Duration::from_millis(i),
        );
        hud.update(&mut ui, &play);
        ui.frame(now.as_secs_f64());
    }
    println!(
        "  hud: update + bind + paint {:.3} ms per frame",
        started.elapsed().as_secs_f64() * 10.0
    );
    hud.update(&mut ui, &play);
    // Atlas uploads are incremental and only this frame reaches the GPU.
    ui.reset_glyph_atlas();
    let mut list = ui.frame(now.as_secs_f64()).clone();
    let world_ui = play.world_ui();
    overlay::draw(&mut list, screen.scale, &world_ui);
    live.world_ui_input = super::run::world_ui_input(&world_ui, screen.scale, me);
    live.eye = Some(screen.camera.eye);
    renderer.set_cutaway(
        (!close_camera)
            .then(|| live.cutaway_subjects())
            .into_iter()
            .flatten(),
    );
    live.draw_world_ui(&mut renderer, &gpu, &screen.camera, &loaded.scene, 12.0);
    let pings = play.minimap_pings(&loaded.scene.colors);
    live.draw_minimap(
        &mut renderer,
        &screen.camera,
        &loaded.scene,
        super::run::MINIMAP_PIXELS,
        &pings,
    );
    let subject = world_ui
        .selected
        .first()
        .and_then(|&id| live.portrait(id, 12.0));
    renderer.set_portrait(&gpu, subject, super::run::PORTRAIT_PIXELS);
    if let (Some(ghost), Some(def)) = (&world_ui.ghost, &world_ui.ghost_definition) {
        let mut instances = live.instances().to_vec();
        if let Some(i) = super::run::ghost_instance(
            &content,
            &newest,
            ghost,
            def,
            terrain,
            me,
            &loaded.scene.colors,
        ) {
            instances.push(i);
        }
        renderer.set_actors(&instances);
    }
    let capture = Capture::new(&gpu.device, size[0], size[1]);
    let layer = renderer.ui_mut(&gpu);
    layer.submit(&gpu, &list);
    layer.set_target(Some(capture.ui_view.clone()));
    let copy = |encoder: &mut wgpu::CommandEncoder| capture.copy(encoder);
    renderer.render(
        &gpu,
        &screen.camera,
        options.time,
        &capture.view,
        Some(&copy),
    );
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    let pixels = capture.read(&gpu.device)?;
    if std::env::var_os("CANOPY_SHOT_HUD").is_some()
        && let Some(texture) = renderer.minimap_texture()
    {
        let image = Capture::new(&gpu.device, 256, 256);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        image.copy_from(&mut encoder, texture);
        gpu.queue.submit([encoder.finish()]);
        let rgba = image.read(&gpu.device)?;
        image::save_buffer(
            out.with_extension("minimap.png"),
            &rgba,
            256,
            256,
            image::ExtendedColorType::Rgba8,
        )?;
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    image::save_buffer(
        out,
        &pixels,
        size[0],
        size[1],
        image::ExtendedColorType::Rgba8,
    )
    .with_context(|| out.display().to_string())?;
    println!(
        "{}: {map} tick {} · selected {:?} · sent {:?}",
        out.display(),
        game.world.tick(),
        play.selection.ids(),
        play.sent.iter().map(|(_, a)| a).collect::<Vec<_>>()
    );
    Ok(())
}
