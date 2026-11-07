//! What a match can reach: every element its races, map, world and rules lead to, by
//! following references, so a match loads only the effects it can play.
//!
//! The walk is generic: an element's references are the strings anywhere in it (map keys
//! too), and any string, or any id-like word inside one (a mission script's), that names
//! a definition, ability, effect, research, loot pool, race or render asset leads to that
//! element. Nothing here knows which fields refer to what, so new fields are followed as
//! soon as they hold ids; a word that only looks like an id reaches a little more than
//! needed, never less.

use crate::{Content, game::Rules, map::UtcMap};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// The elements a match reaches, by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reach {
    /// Every element reached.
    pub elements: BTreeSet<String>,
    /// The effects among them.
    pub effects: BTreeSet<String>,
}

#[derive(Clone, Copy)]
enum Node<'a> {
    Definition(usize),
    Ability(usize),
    Effect(usize),
    Research(&'a crate::game::Research),
    LootPool(&'a crate::game::LootPool),
    Race(&'a str),
    RenderAsset(usize),
}

/// Follows every reference of `content` from the roots: values whose strings are
/// references (a map, a world's state) and element ids.
pub struct Walk<'a> {
    content: &'a Content,
    index: BTreeMap<&'a str, Node<'a>>,
    reach: Reach,
    queue: Vec<Node<'a>>,
}

impl<'a> Walk<'a> {
    pub fn new(content: &'a Content) -> Self {
        let mut index = BTreeMap::new();
        // Render assets first: an element with the same id is the one that counts.
        for (i, a) in content.render_assets.iter().enumerate() {
            index.insert(a.id.as_str(), Node::RenderAsset(i));
        }
        for (id, research) in &content.rules.research {
            index.insert(id.as_str(), Node::Research(research));
        }
        for (id, pool) in &content.rules.loot_pools {
            index.insert(id.as_str(), Node::LootPool(pool));
        }
        for id in content.rules.races.keys() {
            index.insert(id.as_str(), Node::Race(id.as_str()));
        }
        for (i, e) in content.effects.iter().enumerate() {
            index.insert(e.id.as_str(), Node::Effect(i));
        }
        for (i, a) in content.abilities.abilities.iter().enumerate() {
            index.insert(a.id.as_str(), Node::Ability(i));
        }
        for (i, d) in content.definitions.iter().enumerate() {
            index.insert(d.id.as_str(), Node::Definition(i));
        }
        Self {
            content,
            index,
            reach: Reach::default(),
            queue: Vec::new(),
        }
    }

    /// Reaches the element `id` names (nothing when it names none).
    pub fn id(&mut self, id: &str) -> &mut Self {
        self.word(id);
        self.run();
        self
    }

    /// Reaches everything `value` refers to.
    pub fn value<T: Serialize + ?Sized>(&mut self, value: &T) -> &mut Self {
        self.collect(value);
        self.run();
        self
    }

    /// Reaches what the rules every match plays by refer to (level-ups, loot marks,
    /// experience…), leaving out the races, research and loot pools they hold, which
    /// count only once something refers to them.
    pub fn rules(&mut self) -> &mut Self {
        let rules: &Rules = &self.content.rules;
        if let Ok(serde_json::Value::Object(mut fields)) = serde_json::to_value(rules) {
            for held in [
                "races",
                "research",
                "lootPools",
                "defaultRace",
                "startingSetup",
            ] {
                fields.remove(held);
            }
            self.value(&fields);
        }
        self
    }

    /// Everything reached so far.
    pub fn finish(self) -> Reach {
        self.reach
    }

    fn run(&mut self) {
        while let Some(node) = self.queue.pop() {
            let content = self.content;
            match node {
                Node::Definition(i) => {
                    self.collect(&content.definitions[i]);
                    self.collect(&content.looks[i]);
                }
                Node::Ability(i) => self.collect(&content.abilities.abilities[i]),
                Node::Effect(i) => self.collect(&content.effects[i]),
                Node::Research(r) => self.collect(r),
                Node::LootPool(p) => self.collect(p),
                Node::Race(id) => {
                    if let Some(profile) = content.rules.race(id) {
                        self.collect(profile.race);
                        self.collect(profile.starting_setup);
                    }
                }
                Node::RenderAsset(i) => self.collect(&content.render_assets[i]),
            }
        }
    }

    fn collect<T: Serialize + ?Sized>(&mut self, value: &T) {
        let mut strings = Strings(Vec::new());
        let _ = value.serialize(&mut strings);
        for s in std::mem::take(&mut strings.0) {
            self.text(&s);
        }
    }

    /// A string: itself, and every id-like word in it (scripts name ids in quotes).
    fn text(&mut self, text: &str) {
        if self.word(text) {
            return;
        }
        if text.bytes().all(id_byte) {
            return;
        }
        for word in text.split(|c: char| !c.is_ascii() || !id_byte(c as u8)) {
            // A sentence's full stop is no part of the id it ends with.
            if !word.is_empty() && !self.word(word) {
                self.word(word.trim_end_matches(['.', '-', '_']));
            }
        }
    }

    /// Reaches the element `word` names; whether it names one.
    fn word(&mut self, word: &str) -> bool {
        let Some((&id, &node)) = self.index.get_key_value(word) else {
            return false;
        };
        if self.reach.elements.insert(id.to_string()) {
            if matches!(node, Node::Effect(_)) {
                self.reach.effects.insert(id.to_string());
            }
            self.queue.push(node);
        }
        true
    }
}

fn id_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')
}

/// The elements a match on `map` reaches: what the rules always play, the races in play
/// (`races`), the map's placements, camps and mission, and `world` (the match's state:
/// its entities, inventories, statuses and research, whether new, restored or carried
/// over from a campaign).
pub fn of_match<W: Serialize + ?Sized>(
    content: &Content,
    map: &UtcMap,
    races: &[&str],
    world: &W,
) -> Reach {
    let _span = tracing::info_span!("effect_reach").entered();
    let mut walk = Walk::new(content);
    walk.rules();
    for race in races {
        if content.rules.race(race).is_none() {
            // A race the rules do not hold plays the global starting setup.
            walk.value(&content.rules.starting_setup);
        }
        walk.id(race);
    }
    // The map as it plays: the terrain and scenery it was authored with refer to nothing
    // a match plays.
    walk.value(&map.entities)
        .value(&map.camps)
        .value(&map.player_starts)
        .value(&map.stamps)
        .value(&map.mission)
        .value(&map.music);
    walk.value(world);
    walk.finish()
}

/// A serializer that keeps every string and map key of a value and nothing else.
struct Strings(Vec<String>);

#[derive(Debug)]
struct Never;

impl std::fmt::Display for Never {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unreachable")
    }
}

impl std::error::Error for Never {}

impl serde::ser::Error for Never {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Never
    }
}

type Done = Result<(), Never>;

impl serde::Serializer for &mut Strings {
    type Ok = ();
    type Error = Never;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, _: bool) -> Done {
        Ok(())
    }
    fn serialize_i8(self, _: i8) -> Done {
        Ok(())
    }
    fn serialize_i16(self, _: i16) -> Done {
        Ok(())
    }
    fn serialize_i32(self, _: i32) -> Done {
        Ok(())
    }
    fn serialize_i64(self, _: i64) -> Done {
        Ok(())
    }
    fn serialize_u8(self, _: u8) -> Done {
        Ok(())
    }
    fn serialize_u16(self, _: u16) -> Done {
        Ok(())
    }
    fn serialize_u32(self, _: u32) -> Done {
        Ok(())
    }
    fn serialize_u64(self, _: u64) -> Done {
        Ok(())
    }
    fn serialize_f32(self, _: f32) -> Done {
        Ok(())
    }
    fn serialize_f64(self, _: f64) -> Done {
        Ok(())
    }
    fn serialize_char(self, _: char) -> Done {
        Ok(())
    }
    fn serialize_str(self, v: &str) -> Done {
        self.0.push(v.to_owned());
        Ok(())
    }
    fn serialize_bytes(self, _: &[u8]) -> Done {
        Ok(())
    }
    fn serialize_none(self) -> Done {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Done {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Done {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Done {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Done {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Done {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        v: &T,
    ) -> Done {
        v.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, Never> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, Never> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, Never> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Never> {
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, Never> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Never> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self, Never> {
        Ok(self)
    }
}

impl serde::ser::SerializeSeq for &mut Strings {
    type Ok = ();
    type Error = Never;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Done {
        v.serialize(&mut **self)
    }
    fn end(self) -> Done {
        Ok(())
    }
}

impl serde::ser::SerializeTuple for &mut Strings {
    type Ok = ();
    type Error = Never;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Done {
        v.serialize(&mut **self)
    }
    fn end(self) -> Done {
        Ok(())
    }
}

impl serde::ser::SerializeTupleStruct for &mut Strings {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Done {
        v.serialize(&mut **self)
    }
    fn end(self) -> Done {
        Ok(())
    }
}

impl serde::ser::SerializeTupleVariant for &mut Strings {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Done {
        v.serialize(&mut **self)
    }
    fn end(self) -> Done {
        Ok(())
    }
}

impl serde::ser::SerializeMap for &mut Strings {
    type Ok = ();
    type Error = Never;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> Done {
        k.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Done {
        v.serialize(&mut **self)
    }
    fn end(self) -> Done {
        Ok(())
    }
}

impl serde::ser::SerializeStruct for &mut Strings {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Done {
        v.serialize(&mut **self)
    }
    fn end(self) -> Done {
        Ok(())
    }
}

impl serde::ser::SerializeStructVariant for &mut Strings {
    type Ok = ();
    type Error = Never;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> Done {
        v.serialize(&mut **self)
    }
    fn end(self) -> Done {
        Ok(())
    }
}
