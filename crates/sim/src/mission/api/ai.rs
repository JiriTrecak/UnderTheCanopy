//! The campaign AI's part of the `mission` table (docs/campaign.md "Formats", script
//! additions): `ai_waves`, `ai_attack_now`, `ai_target`, `ai_behaviour`, `ai_wake`.
//!
//! A player is its number (`2`) or owner (`"player.2"`) and must be one of the mission's
//! computer players.

use super::{Api, Args, MissionEffect, Reply, none, one, text};
use crate::{ai::campaign::Order, mission::sandbox::Scalar};
use canopy_content::{
    map::{Behaviour, WaveTarget},
    placement::{Activation, Owner},
};

impl Api<'_> {
    pub(super) fn ai(&mut self, name: &str, a: &Args) -> Reply {
        let player = self.computer(a.get(0))?;
        let order = match name {
            "ai_waves" => match a.get(1) {
                Scalar::Boolean(on) => Order::Waves(*on),
                _ => return Err("ai_waves expects true or false".into()),
            },
            "ai_attack_now" => Order::AttackNow,
            "ai_target" => {
                let target = text(a.get(1), 120)?;
                Order::Target(match target {
                    "base" => WaveTarget::Base,
                    "nearest" => WaveTarget::Nearest,
                    region => {
                        self.script
                            .region(region)
                            .ok_or_else(|| format!("Unknown region: {region}"))?;
                        WaveTarget::Region(region.to_string())
                    }
                })
            }
            "ai_behaviour" => {
                let behaviour = text(a.get(1), 40)?;
                Order::Behaviour(
                    Behaviour::parse(behaviour)
                        .ok_or("Behaviour must be passive, base, waves or skirmish")?,
                )
            }
            "ai_wake" => return self.wake(player),
            _ => return Err(format!("Unknown mission function {name}")),
        };
        self.effects.push(MissionEffect::Ai { player, order });
        none()
    }

    /// A dormant player arrives: its script-activated placements appear and it starts
    /// playing. False when it is not dormant.
    fn wake(&mut self, player: u8) -> Reply {
        let dormant = self
            .state
            .campaign_ai
            .iter()
            .any(|m| m.player == player && m.dormant);
        if !dormant {
            return one(Scalar::Boolean(false));
        }
        let owner = Owner::Player(player);
        let arriving: Vec<String> = self
            .script
            .placements()
            .iter()
            .filter(|p| p.owner == owner && p.activation == Some(Activation::Script))
            .filter(|p| !self.draft.spawned.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
        for id in arriving {
            let placement = self.script.placement(&id).expect("listed placement");
            self.placed(placement)?;
            self.draft.spawned.push(id.clone());
            let camp = self.script.camp_of(&id).map(|c| c.id.clone());
            self.effects.push(MissionEffect::Spawn {
                placement: id,
                camp,
            });
        }
        self.effects.push(MissionEffect::Ai {
            player,
            order: Order::Wake,
        });
        one(Scalar::Boolean(true))
    }

    /// A computer player of the mission, zero-based.
    fn computer(&self, raw: &Scalar) -> Result<u8, String> {
        let number = match raw {
            Scalar::Integer(n) => u8::try_from(*n).ok(),
            Scalar::String(s) => s.strip_prefix("player.").and_then(|n| n.parse::<u8>().ok()),
            _ => None,
        };
        number
            .filter(|n| self.script.mission.players.iter().any(|p| p.player == *n))
            .map(|n| n - 1)
            .ok_or_else(|| "Expected a computer player of this mission".into())
    }
}
