//! The campaign's part of the `mission` table (docs/campaign.md "Mission script
//! additions"): `campaign_get(name)`, `campaign_set(name, value)`, `allow(id)` and
//! `forbid(id)`.
//!
//! Like every call, they change nothing until the run succeeds: they queue
//! [`crate::campaign::Change`]s, and `campaign_get` reads the run's own earlier sets.

use super::{Api, Args, Reply, none, one, text};
use crate::{
    campaign::{self, Change},
    mission::{effect::MissionEffect, sandbox::Scalar, state::Variable},
};
use canopy_content::{Id, game::Kind};

/// The names this part answers, in the order `NAMES` lists them.
pub const NAMES: [&str; 4] = ["campaign_get", "campaign_set", "allow", "forbid"];

impl Api<'_> {
    pub(super) fn campaign(&mut self, name: &str, a: &Args) -> Reply {
        match name {
            "campaign_get" => self.campaign_get(a),
            "campaign_set" => self.campaign_set(a),
            "allow" => {
                let id = self.tech(a)?;
                self.effect(MissionEffect::Campaign(Change::Allow(id)))
            }
            "forbid" => {
                let id = self.tech(a)?;
                self.effect(MissionEffect::Campaign(Change::Forbid(id)))
            }
            _ => Err(format!("Unknown mission function {name}")),
        }
    }

    /// The value as this run left it, else as the campaign holds it.
    fn campaign_value(&self, key: &str) -> Option<Variable> {
        let queued = self.effects.iter().rev().find_map(|e| match e {
            MissionEffect::Campaign(Change::Set { name, value }) if name == key => {
                Some(value.clone())
            }
            _ => None,
        });
        match queued {
            Some(value) => value,
            None => self
                .state
                .campaign
                .as_ref()
                .and_then(|c| c.variables.get(key).cloned()),
        }
    }

    fn campaign_get(&self, a: &Args) -> Reply {
        let key = text(a.get(0), 120)?;
        one(match self.campaign_value(key) {
            None => Scalar::Nil,
            Some(Variable::String(s)) => Scalar::String(s),
            Some(Variable::Integer(i)) => Scalar::Integer(i),
            Some(Variable::Number(n)) => Scalar::Number(n),
            Some(Variable::Boolean(b)) => Scalar::Boolean(b),
        })
    }

    fn campaign_set(&mut self, a: &Args) -> Reply {
        let name = text(a.get(0), 120)?.to_string();
        let value = match a.get(1) {
            Scalar::Nil => None,
            Scalar::String(s) => Some(Variable::String(s.clone())),
            Scalar::Integer(i) => Some(Variable::Integer(*i)),
            Scalar::Number(n) => Some(Variable::Number(*n)),
            Scalar::Boolean(b) => Some(Variable::Boolean(*b)),
        };
        if let Some(value) = &value {
            // Count the variables as this run has left them.
            let mut variables = self
                .state
                .campaign
                .as_ref()
                .map(|c| c.variables.clone())
                .unwrap_or_default();
            for effect in &self.effects {
                if let MissionEffect::Campaign(Change::Set { name, value }) = effect {
                    match value {
                        Some(v) => {
                            variables.insert(name.clone(), v.clone());
                        }
                        None => {
                            variables.remove(name);
                        }
                    }
                }
            }
            campaign::check_variable(&variables, &name, value)?;
        }
        self.effects
            .push(MissionEffect::Campaign(Change::Set { name, value }));
        none()
    }

    /// A unit, building or item definition, or a research.
    fn tech(&self, a: &Args) -> Result<Id, String> {
        let id = text(a.get(0), 120)?;
        let known = self.catalog.find(id).is_some_and(|d| {
            matches!(
                self.catalog.kind(d),
                Kind::Unit | Kind::Building | Kind::Item
            )
        }) || self.catalog.content.rules.research.contains_key(id);
        if !known {
            return Err(format!("Unknown tech: {id}"));
        }
        Id::new(id).map_err(|_| format!("Unknown tech: {id}"))
    }
}
