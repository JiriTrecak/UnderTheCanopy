//! Attack waves: data-driven lines of waves (`mission.players[].ai.waves`). A wave is
//! trained free at the player's own buildings (or arrives at a region), gathers until its
//! last unit is out, then marches on its target with the player's heroes when the line
//! says so. Under way it keeps attacking until its units are gone; its heroes then walk
//! home.

use super::{Memory, Play, Purpose, Wave, living, seconds};
use crate::{catalog::Def, entity::Milli};
use canopy_content::map::{Behaviour, WaveSource};

impl Play<'_, '_> {
    /// Sends the waves that are due (Waves behaviour only; timers keep running otherwise).
    pub(super) fn lines(&mut self, m: &mut Memory) {
        let attack_now = std::mem::take(&mut m.attack_now);
        if m.behaviour != Behaviour::Waves {
            return;
        }
        let tick = self.tick;
        let mut sent_any = false;
        let plan = self.plan;
        for index in 0..m.lines.len() {
            let line = &plan.ai.waves[index];
            let state = &mut m.lines[index];
            if state.done {
                continue;
            }
            if line.until.is_some_and(|u| tick >= m.since + seconds(u)) {
                state.done = true;
                continue;
            }
            let due = m.waves_on && state.next.is_some_and(|n| n <= tick);
            if due || (attack_now && state.next.is_some()) {
                self.send_wave(m, index);
                sent_any = true;
            }
        }
        if attack_now
            && !sent_any
            && let Some(index) = m.lines.iter().position(|l| !l.done)
        {
            self.send_wave(m, index);
        }
    }

    /// Starts the line's next wave and schedules the one after.
    fn send_wave(&mut self, m: &mut Memory, index: usize) {
        let plan = self.plan;
        let line = &plan.ai.waves[index];
        let tick = self.tick;
        let sent = m.lines[index].sent;
        {
            let state = &mut m.lines[index];
            state.sent += 1;
            match line.every {
                Some(every) => state.next = Some(tick + seconds(every)),
                None => state.done = true,
            }
        }
        let mut units: Vec<Def> = Vec::new();
        for u in &line.units {
            if let Some(def) = self.find(u.unit.as_str()) {
                units.extend(std::iter::repeat_n(def, u.count as usize));
            }
        }
        if let Some(grow) = &line.grow
            && let Some(def) = self.find(grow.unit.as_str())
        {
            let extra = sent / grow.every.max(1) * grow.count;
            units.extend(std::iter::repeat_n(def, extra as usize));
        }
        let id = m.next_wave;
        m.next_wave += 1;
        let mut wave = Wave {
            id,
            line: index as u32,
            units: Vec::new(),
            heroes: Vec::new(),
            pending: 0,
            launched: false,
            target: m.target.clone().unwrap_or_else(|| line.target.clone()),
        };
        match &line.from {
            WaveSource::Trained => {
                for def in units {
                    if let Some(training) = self.train(m, def, Purpose::Wave(id)) {
                        m.training.push(training);
                        wave.pending += 1;
                    }
                }
            }
            WaveSource::Region(region) => {
                let Some(at) = self.campaign.region(region) else {
                    return;
                };
                let mut taken = Vec::new();
                for def in units {
                    if let Some(spot) = self.spot_near(def, at, &taken) {
                        taken.push(spot);
                        wave.units.push(self.spawn_unit(def, spot));
                    }
                }
            }
        }
        if wave.pending > 0 || !wave.units.is_empty() {
            m.waves.push(wave);
        }
    }

    /// Launches gathered waves and keeps those under way attacking.
    pub(super) fn steer_waves(&mut self, m: &mut Memory) {
        let mut busy_heroes: Vec<_> = m.waves.iter().flat_map(|w| w.heroes.clone()).collect();
        for index in 0..m.waves.len() {
            let mut wave = m.waves[index].clone();
            wave.units.retain(|&id| self.own_unit(id).is_some());
            wave.heroes.retain(|&id| self.own_unit(id).is_some());
            if !wave.launched {
                if wave.pending > 0 {
                    m.waves[index] = wave;
                    continue;
                }
                wave.launched = true;
                if self.plan.ai.waves[wave.line as usize].hero {
                    let heroes = self.heroes(&busy_heroes);
                    busy_heroes.extend(&heroes);
                    wave.heroes.extend(heroes);
                }
            }
            if wave.units.is_empty() {
                // Over: its heroes walk home.
                for &hero in &wave.heroes {
                    let home = m
                        .posts
                        .iter()
                        .position(|p| p.entity == Some(hero))
                        .map(|p| (p, m.posts[p].home));
                    if let Some((post, home)) = home {
                        m.posts[post].sent = self.send(hero, home, true);
                    }
                }
                wave.heroes.clear();
            } else {
                let centre = self.centre(&wave.units);
                let target = self.resolve(&wave.target, centre);
                for &id in wave.units.iter().chain(&wave.heroes) {
                    let Some(e) = self.own_unit(id) else {
                        continue;
                    };
                    let idle = e
                        .unit
                        .as_ref()
                        .is_some_and(|u| u.order.is_none() && u.target.is_none());
                    let from = e.position;
                    if !idle {
                        continue;
                    }
                    let to = match wave.target {
                        canopy_content::map::WaveTarget::Nearest => {
                            self.resolve(&wave.target, from)
                        }
                        _ => target,
                    };
                    if let Some(to) = to {
                        self.send(id, to, true);
                    }
                }
            }
            m.waves[index] = wave;
        }
        m.waves.retain(|w| !w.launched || !w.units.is_empty());
    }

    /// Our living heroes not marching with another wave, by id.
    fn heroes(&self, busy: &[crate::entity::EntityId]) -> Vec<crate::entity::EntityId> {
        let catalog = self.ctx.catalog;
        self.ctx
            .index
            .units
            .iter()
            .filter_map(|&id| self.ctx.state.entities.get(id))
            .filter(|e| {
                e.owner == self.me
                    && living(e)
                    && catalog.get(e.definition).is_hero()
                    && e.unit.as_ref().is_some_and(|u| u.contained.is_none())
                    && !busy.contains(&e.id)
            })
            .map(|e| e.id)
            .collect()
    }

    /// The average position of living units.
    fn centre(&self, units: &[crate::entity::EntityId]) -> Milli {
        let (mut x, mut y, mut n) = (0i64, 0i64, 0i64);
        for &id in units {
            if let Some(e) = self.ctx.state.entities.get(id) {
                x += i64::from(e.position.x);
                y += i64::from(e.position.y);
                n += 1;
            }
        }
        let n = n.max(1);
        Milli {
            x: (x / n) as i32,
            y: (y / n) as i32,
        }
    }
}
