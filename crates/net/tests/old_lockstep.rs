//! The Rust outbox and mailbox against the old TypeScript `Lockstep` and `Room` on the same
//! long random operation sequences: every message must be byte-identical.
//!
//! Skipped without the old repository, its `node_modules` and node (CI).

mod fixture;

use canopy_net::lockstep::Outbox;
use canopy_net::mailbox::Mailbox;
use canopy_net::wire::{Bundle, ServerMsg};
use canopy_sim::Action;
use serde_json::{Value, json};
use std::process::Command;

/// A small deterministic generator (xorshift32).
struct Dice(u32);

impl Dice {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }

    /// `base + [-down, up]`, never negative.
    fn around(&mut self, base: u32, down: u32, up: u32) -> u32 {
        (base + self.below(down + up + 1)).saturating_sub(down)
    }
}

/// An action as the JSON text this client writes (JSON key order is the sender's own; the
/// old client's call sites each have theirs, and nothing downstream depends on it).
fn text(action: Value) -> Value {
    let action: Action = serde_json::from_value(action).unwrap();
    Value::String(serde_json::to_string(&action).unwrap())
}

fn action(dice: &mut Dice) -> Value {
    text(match dice.below(10) {
        0 => json!({"type": "noop"}),
        1 => json!({"type": "stop", "actors": []}),
        2 => {
            json!({"type": "move", "actors": [1 + dice.below(30), 40], "destination": {"x": dice.below(2048), "y": 5}, "attackMove": true})
        }
        _ => json!({"type": "stop", "actors": [1 + dice.below(50)]}),
    })
}

fn outbox_ops(dice: &mut Dice) -> Vec<Value> {
    let mut ops = Vec::new();
    let mut through = 0;
    for step in 0..600 {
        if step == 300 {
            // More than one turn's worth at once.
            for _ in 0..70 {
                ops.push(
                    json!({"op": "send", "action": text(json!({"type": "stop", "actors": [7]}))}),
                );
            }
        }
        if dice.below(10) < 4 {
            let mut a = action(dice);
            if a == text(json!({"type": "stop", "actors": []})) {
                a = text(json!({"type": "stop", "actors": [3]}));
            }
            ops.push(json!({"op": "send", "action": a}));
        } else {
            through = dice.around(through, 3, 5);
            let bundle_tick = if dice.below(2) == 0 {
                through + 1
            } else {
                dice.around(through, 2, 2)
            };
            ops.push(json!({"op": "confirm", "through": through, "bundleTick": bundle_tick}));
        }
    }
    ops
}

fn mailbox_ops(dice: &mut Dice, players: &[u8], delay: u32) -> Vec<Value> {
    let mut ops = Vec::new();
    let mut through = vec![0u32; players.len()];
    for step in 0..900 {
        if step == 600 {
            ops.push(json!({"op": "drop", "player": players[2]}));
        }
        let i = dice.below(players.len() as u32) as usize;
        let t = if dice.below(40) == 0 {
            5000
        } else {
            dice.around(through[i], 2, 4)
        };
        through[i] = through[i].max(t.min(4000));
        let bundles: Vec<Value> = (0..dice.below(3))
            .map(|_| {
                let actions: Vec<Value> = (0..dice.below(3))
                    .map(|_| action(dice))
                    .filter(|a| *a != text(json!({"type": "noop"})))
                    .collect();
                json!({"tick": dice.around(t, 3, delay + 2), "actions": actions})
            })
            .collect();
        ops.push(json!({"op": "confirm", "player": players[i], "through": t, "bundles": bundles}));
    }
    ops
}

fn rust_outbox(ops: &[Value]) -> Vec<String> {
    let mut outbox = Outbox::default();
    let mut out = Vec::new();
    for op in ops {
        if op["op"] == "send" {
            outbox.send(serde_json::from_str(op["action"].as_str().unwrap()).unwrap());
        } else {
            let through = op["through"].as_u64().unwrap() as u32;
            let tick = op["bundleTick"].as_u64().unwrap() as u32;
            out.extend(outbox.confirm(through, tick).map(|m| m.to_json()));
        }
    }
    out
}

fn rust_mailbox(players: &[u8], delay: u32, ops: &[Value]) -> Vec<String> {
    let mut mailbox = Mailbox::new(players.iter().copied(), delay, true);
    let mut out = Vec::new();
    for op in ops {
        let player = op["player"].as_u64().unwrap() as u8;
        let commits = if op["op"] == "drop" {
            mailbox.drop_player(player)
        } else {
            let bundles: Vec<Bundle> = op["bundles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| Bundle {
                    tick: b["tick"].as_u64().unwrap() as u32,
                    actions: b["actions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|a| serde_json::from_str::<Action>(a.as_str().unwrap()).unwrap())
                        .collect(),
                })
                .collect();
            mailbox.confirm(player, op["through"].as_u64().unwrap() as u32, &bundles)
        };
        out.extend(commits.into_iter().map(|c| {
            serde_json::to_string(&ServerMsg::Commit {
                tick: c.tick,
                slots: c.slots,
            })
            .unwrap()
        }));
    }
    out
}

#[test]
fn outbox_and_mailbox_send_what_the_typescript_ones_send() {
    let Some(repository) = fixture::old_repository() else {
        eprintln!("skipped: the old repository is not available");
        return;
    };
    let players = [2u8, 0, 1];
    let delay = 3;
    let mut dice = Dice(0x5eed_1234);
    let outbox = outbox_ops(&mut dice);
    let mailbox = mailbox_ops(&mut dice, &players, delay);
    let file = std::env::temp_dir().join(format!("canopy-net-trace-{}.json", std::process::id()));
    std::fs::write(
        &file,
        json!({"outbox": outbox, "mailbox": {"players": players, "delay": delay, "ops": mailbox}})
            .to_string(),
    )
    .unwrap();
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ts/trace.mts");
    let output = match Command::new("node")
        .args(["--import", "tsx", script])
        .arg(&repository)
        .arg(&file)
        .current_dir(&repository)
        .output()
    {
        Ok(output) => output,
        Err(_) => {
            eprintln!("skipped: node is not available");
            return;
        }
    };
    let _ = std::fs::remove_file(&file);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let (ts_outbox, ts_mailbox) = text.split_once("---\n").expect("two sections");
    let ts_outbox: Vec<&str> = ts_outbox.lines().collect();
    let ts_mailbox: Vec<&str> = ts_mailbox.lines().collect();

    let rust = rust_outbox(&outbox);
    assert!(rust.len() > 100, "a long trace");
    assert_eq!(rust.len(), ts_outbox.len());
    for (i, (r, t)) in rust.iter().zip(&ts_outbox).enumerate() {
        assert_eq!(r, t, "outbox message {i}");
    }
    let rust = rust_mailbox(&players, delay, &mailbox);
    assert!(rust.len() > 100, "a long trace");
    assert!(rust.iter().any(|c| c.contains("\"move\"")));
    assert_eq!(rust.len(), ts_mailbox.len());
    for (i, (r, t)) in rust.iter().zip(&ts_mailbox).enumerate() {
        assert_eq!(r, t, "commit {i}");
    }
}
