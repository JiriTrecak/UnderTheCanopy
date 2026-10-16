//! Sample HUD and gallery content shared by the `gallery` and `bench` examples.
//! (Example data, not shipped content: the game fills the HUD from its own models.)

use canopy_ui::{Color, Cost, ImageKey, NodeId, Tooltip, Ui};

pub fn icon(name: &str) -> String {
    format!("{name}/image.png")
}

pub const ICONS: &[&str] = &[
    "asset.icons.command-move",
    "asset.icons.command-stop",
    "asset.icons.command-hold",
    "asset.icons.command-attack",
    "asset.icons.command-patrol",
    "asset.icons.command-rally",
    "asset.icons.spell-holy-light",
    "asset.icons.spell-entangling-roots",
    "asset.icons.spell-dispel-magic",
    "asset.icons.command-build",
    "asset.icons.command-build-advanced",
    "asset.icons.command-cancel",
    "asset.icons.item-amber-carapace",
    "asset.icons.item-moon-dew",
    "asset.icons.item-thornband",
    "asset.icons.item-firefly-pendant",
    "asset.icons.item-resin-salve",
    "asset.icons.resource-amber",
    "asset.icons.resource-lumber",
    "asset.icons.resource-root",
    "asset.icons.resource-supply",
    "asset.icons.stat-armor-light",
    "asset.icons.unit-ants-hunter",
    "asset.custom.roar-command-icon",
    "asset.custom.web-command-icon",
    "asset.custom.avatar-command-icon",
];

fn key(ui: &mut Ui, name: &str) -> ImageKey {
    ui.image_key(&icon(name))
}

fn find(ui: &Ui, path: &str) -> NodeId {
    ui.find(path).unwrap_or_else(|| panic!("no node {path}"))
}

fn sub(ui: &Ui, base: NodeId, rel: &str) -> NodeId {
    ui.find_in(base, rel)
        .unwrap_or_else(|| panic!("no node {rel}"))
}

pub struct Command {
    pub icon: &'static str,
    pub name: &'static str,
    pub key: &'static str,
    pub body: &'static str,
    pub costs: &'static [(&'static str, &'static str)],
}

pub const COMMANDS: [Command; 12] = [
    Command {
        icon: "asset.icons.command-move",
        name: "Move",
        key: "Q",
        body: "Move to the target area, ignoring enemies on the way.",
        costs: &[],
    },
    Command {
        icon: "asset.icons.command-stop",
        name: "Stop",
        key: "W",
        body: "Stop moving and give up the current order.",
        costs: &[],
    },
    Command {
        icon: "asset.icons.command-hold",
        name: "Hold Position",
        key: "E",
        body: "Stay in place and attack enemies in range without chasing them.",
        costs: &[],
    },
    Command {
        icon: "asset.icons.command-attack",
        name: "Attack",
        key: "R",
        body: "Attack a target, or move to an area attacking every enemy met on the way.\nShift-click to queue.",
        costs: &[],
    },
    Command {
        icon: "asset.icons.command-patrol",
        name: "Patrol",
        key: "A",
        body: "Walk back and forth, engaging enemies that come close.",
        costs: &[],
    },
    Command {
        icon: "asset.icons.command-rally",
        name: "Rally",
        key: "S",
        body: "Set the gathering point for new units.",
        costs: &[],
    },
    Command {
        icon: "asset.icons.spell-holy-light",
        name: "Sunlit Sap",
        key: "D",
        body: "Heals a friendly unit for [b]200[/b] health, or deals half that to undead.",
        costs: &[("asset.icons.resource-amber", "65")],
    },
    Command {
        icon: "asset.icons.spell-entangling-roots",
        name: "Entangling Roots",
        key: "F",
        body: "Roots bind an enemy for 6 seconds, dealing 15 damage per second.\n[color=#ff8a65]Requires level 3.[/color]",
        costs: &[("asset.icons.resource-amber", "75")],
    },
    Command {
        icon: "asset.custom.roar-command-icon",
        name: "Warcry",
        key: "Z",
        body: "Nearby allies deal 25% more damage for 30 seconds.",
        costs: &[("asset.icons.resource-amber", "50")],
    },
    Command {
        icon: "asset.icons.command-build",
        name: "Build Structure",
        key: "X",
        body: "Choose a structure to build.",
        costs: &[],
    },
    Command {
        icon: "asset.icons.command-build-advanced",
        name: "Build Advanced Structure",
        key: "C",
        body: "Choose an advanced structure to build.",
        costs: &[
            ("asset.icons.resource-lumber", "120"),
            ("asset.icons.resource-amber", "80"),
        ],
    },
    Command {
        icon: "asset.icons.command-cancel",
        name: "Cancel",
        key: "V",
        body: "Cancel the current order.",
        costs: &[],
    },
];

/// Fills the HUD with a selected hero, a full command card, inventory and resources.
pub fn populate_hud(ui: &mut Ui) {
    let commands = find(ui, "hud/dock/actions/commands");
    ui.set_count(commands, 12);
    for (i, c) in COMMANDS.iter().enumerate() {
        let cell = ui.child(commands, i).expect("command cell");
        let icon = key(ui, c.icon);
        let costs = c
            .costs
            .iter()
            .map(|(img, amount)| Cost {
                image: Some(key(ui, img)),
                amount: (*amount).into(),
            })
            .collect();
        let i_node = sub(ui, cell, "icon");
        ui.set_image(i_node, Some(icon));
        let k = sub(ui, cell, "key");
        ui.set_text(k, c.key);
        ui.set_hotkey(cell, Some(c.key));
        ui.set_tooltip(
            cell,
            Some(Tooltip {
                title: c.name.into(),
                hotkey: Some(c.key.into()),
                costs,
                body: c.body.into(),
                note: None,
            }),
        );
    }
    // Move is the active order; Entangling Roots is not learned; Sunlit Sap cools down.
    let first = ui.child(commands, 0).expect("cell");
    ui.set_checked(first, true);
    let roots = ui.child(commands, 7).expect("cell");
    ui.set_enabled(roots, false);
    let sap = ui.child(commands, 6).expect("cell");
    let cd = sub(ui, sap, "cooldown");
    ui.set_visible(cd, true);
    ui.set_value(cd, 0.35);
    let secs = sub(ui, cd, "seconds");
    ui.set_text(secs, "4");

    let name = find(ui, "hud/dock/selection/name");
    ui.set_text(name, "Brood Warden");
    let level = find(ui, "hud/dock/selection/level");
    ui.set_text(level, "4");
    let xp = find(ui, "hud/dock/selection/experience");
    ui.set_value(xp, 0.62);
    let portrait = find(ui, "hud/dock/selection/portrait");
    let p = ui.image_key("@portrait");
    ui.set_image(portrait, Some(p));
    let minimap = find(ui, "hud/dock/minimap");
    let m = ui.image_key("@minimap");
    ui.set_image(minimap, Some(m));
    set_vitals(ui, 432, 600, 90, 200);

    let stats = find(ui, "hud/dock/selection/stats");
    let rows = [
        ("asset.icons.command-attack", "Damage", "18–24"),
        ("asset.icons.stat-armor-light", "Armor", "3"),
        ("asset.icons.command-move", "Speed", "270"),
    ];
    ui.set_count(stats, rows.len());
    for (i, (img, label, value)) in rows.iter().enumerate() {
        let s = ui.child(stats, i).expect("stat");
        let k = key(ui, img);
        let n = sub(ui, s, "icon");
        ui.set_image(n, Some(k));
        let n = sub(ui, s, "label");
        ui.set_text(n, label);
        let n = sub(ui, s, "value");
        ui.set_text(n, value);
        ui.set_tooltip(
            s,
            Some(Tooltip {
                title: (*label).into(),
                body: format!("{label}: {value}"),
                ..Tooltip::default()
            }),
        );
    }

    let inventory = find(ui, "hud/dock/selection/inventory");
    let items = [
        ("asset.icons.item-amber-carapace", None),
        ("asset.icons.item-moon-dew", Some("3")),
        ("asset.icons.item-thornband", None),
        ("asset.icons.item-firefly-pendant", None),
        ("asset.icons.item-resin-salve", Some("2")),
        ("asset.custom.web-command-icon", None),
    ];
    ui.set_count(inventory, items.len());
    for (i, (img, count)) in items.iter().enumerate() {
        let slot = ui.child(inventory, i).expect("slot");
        let k = key(ui, img);
        let n = sub(ui, slot, "icon");
        ui.set_image(n, Some(k));
        let n = sub(ui, slot, "count");
        ui.set_visible(n, count.is_some());
        ui.set_text(n, count.unwrap_or(""));
    }

    let resources = find(ui, "hud/dock/actions/resources");
    let rows = [
        ("asset.icons.resource-amber", "1,240", "Amber"),
        ("asset.icons.resource-lumber", "860", "Lumber"),
        ("asset.icons.resource-root", "75", "Roots"),
        ("asset.icons.resource-supply", "38/60", "Supply"),
    ];
    ui.set_count(resources, rows.len());
    for (i, (img, value, name)) in rows.iter().enumerate() {
        let r = ui.child(resources, i).expect("resource");
        let k = key(ui, img);
        let n = sub(ui, r, "icon");
        ui.set_image(n, Some(k));
        let n = sub(ui, r, "value");
        ui.set_text(n, value);
        ui.set_tooltip(
            r,
            Some(Tooltip {
                title: (*name).into(),
                ..Tooltip::default()
            }),
        );
    }

    let heroes = find(ui, "hud/heroes");
    let list = [
        ("asset.icons.unit-ants-hunter", 1.0, true),
        ("asset.custom.avatar-command-icon", 0.35, true),
        ("asset.custom.roar-command-icon", 0.0, false),
    ];
    ui.set_count(heroes, list.len());
    for (i, (img, hp, alive)) in list.iter().enumerate() {
        let h = ui.child(heroes, i).expect("hero");
        let k = key(ui, img);
        let n = sub(ui, h, "portrait");
        ui.set_image(n, Some(k));
        let bar = sub(ui, h, "health");
        ui.set_value(bar, *hp);
        ui.set_color(bar, Some(health_color(*hp)));
        let n = sub(ui, h, "key");
        ui.set_text(n, &format!("F{}", i + 1));
        ui.set_hotkey(h, Some(&format!("F{}", i + 1)));
        ui.set_enabled(h, *alive);
        ui.set_checked(h, i == 0);
        ui.set_tooltip(
            h,
            Some(Tooltip {
                title: format!("Hero {}", i + 1),
                hotkey: Some(format!("F{}", i + 1)),
                body: "Click to select. Double-click to center the camera.".into(),
                ..Tooltip::default()
            }),
        );
    }
}

/// The old game's health colour ramp: red → yellow → green.
pub fn health_color(ratio: f32) -> Color {
    let (a, b, t) = if ratio < 0.5 {
        (0xdb3935u32, 0xe8bf32u32, ratio * 2.0)
    } else {
        (0xe8bf32, 0x64cc39, (ratio - 0.5) * 2.0)
    };
    let ch =
        |s: u32| (((a >> s) & 255) as f32 * (1.0 - t) + ((b >> s) & 255) as f32 * t).round() as u32;
    Color::hex((ch(16) << 16) | (ch(8) << 8) | ch(0))
}

pub fn set_vitals(ui: &mut Ui, hp: u32, max_hp: u32, mana: u32, max_mana: u32) {
    let health = find(ui, "hud/dock/selection/health");
    let ratio = hp as f32 / max_hp as f32;
    ui.set_value(health, ratio);
    ui.set_color(health, Some(health_color(ratio)));
    let v = sub(ui, health, "value");
    ui.set_text(v, &format!("{hp} / {max_hp}"));
    let m = find(ui, "hud/dock/selection/mana");
    ui.set_value(m, mana as f32 / max_mana as f32);
    let v = sub(ui, m, "value");
    ui.set_text(v, &format!("{mana} / {max_mana}"));
}

/// A chat and event log of `n` lines with markup.
pub fn populate_log(ui: &mut Ui, list: &str, n: usize) {
    let log = find(ui, list);
    ui.set_count(log, n);
    let names = [
        "[color=#8fd3ff]Jiri[/color]",
        "[color=#ffb86b]Moss[/color]",
        "[color=#c3a6ff]Amber AI[/color]",
    ];
    for i in 0..n {
        let line = ui.child(log, i).expect("line");
        let text = match i % 5 {
            0 => format!(
                "[b]{:02}:{:02}[/b]  Your hero reached level {}.",
                i / 3,
                (i * 7) % 60,
                2 + i % 8
            ),
            1 => format!(
                "{}: anyone up for a rematch on Turtle Rock? #{i}",
                names[i % 3]
            ),
            2 => format!(
                "Press [key]F{}[/key] to select hero {}.",
                1 + i % 3,
                1 + i % 3
            ),
            3 => format!(
                "[color=#ff8a65]Your base is under attack[/color] near the eastern ford (line {i})."
            ),
            _ => format!(
                "{}: gathering [i]amber[/i] — 38/60 supply, {} lumber",
                names[(i + 1) % 3],
                100 + i * 7
            ),
        };
        ui.set_text(line, &text);
    }
}
