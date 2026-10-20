"""Shared weapon flights and the forest creatures' ranged attacks:

    python3 tools/content/weapon_fx.py

Writes base/effects/weapons/*-flight.json and *-hit.json (any race's thorns, silk and stones
use them) and base/effects/neutral/*, and points every ranged forest creature at its flight
and impact (`presentation.launchEffect`, `impactEffect` of base/units/neutral/*). Physical
shots keep their projectile model and trail a faint wake; magical creatures fire a glowing
mote in their own colour with a solid trail and burst where it lands.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from vfx_layers import *  # noqa: E402,F401,F403

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE = os.path.join(ROOT, 'base')
# The thorn, arrow, silk and stone projectiles' arc (their effects' arcHeight).
ARC = 0.65


def write(rel, doc):
    path = os.path.join(BASE, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w') as f:
        f.write(json.dumps(doc, indent=2, ensure_ascii=False) + '\n')


def effect(id_, name, description, duration, layers):
    return {'id': id_, 'name': name, 'description': description, 'durationTicks': duration, 'loop': False,
            'layers': layers}


def thorn_flight():
    return effect('effect.weapons.thorn-flight', 'Thorn flight',
                  'The air wake of a thrown thorn, needle or arrow: a faint pale streak and a few motes of dust '
                  'left hanging behind it.', FLIGHT, [
                      carrier(ARC),
                      wake('wake', 'path', '#fbf6ea', width=0.42, ticks=8, opacity=0.85),
                      motes('dust', 'path', '#e2d6be', '#cfc2a6', 30, 0.28, 14, additive=False, intensity=0.7,
                            speed=0.4),
                  ])


def silk_flight():
    return effect('effect.weapons.silk-flight', 'Silk flight',
                  'A strand of silk streaming behind a spat glob, sagging as it hangs in the air.', FLIGHT, [
                      carrier(ARC),
                      wake('strand', 'path', '#f6f4ee', width=0.26, ticks=12, opacity=0.9),
                      motes('fibres', 'path', '#ffffff', '#e8e4da', 22, 0.2, 20, additive=False, intensity=0.8,
                            gravity=2.0),
                  ])


def splinter_hit():
    return effect('effect.weapons.splinter-hit', 'Splinter hit',
                  'A thorn or arrow strikes: a puff of dust and a spray of splinters that land.', 30, [
                      flare('flare', '#fff2d6', 1.6, duration=3, intensity=1.3),
                      burst('splinters', '#a88a62', '#7a6448', 8, 0.26, speed=5.5, lifetime=24, gravity=16,
                            additive=False, texture=CHUNK, cone=70, collide=0.8),
                      puff('dust', '#c8b89a', '#b0a084', 4, 0.9, lifetime=24, opacity=0.5),
                  ])


# Magical creatures: (name, colour, light colour, additive share of the trail)
MAGIC = {
    'willow-wisp': ('Willow light', '#a8ffd8', '#effff6', 1.0),
    'dew-sprite': ('Dew bolt', '#6fd6ff', '#e6f8ff', 0.6),
    'bloom-witch': ('Bloom bolt', '#ff6fc8', '#ffe0f4', 1.0),
    'root-seer': ('Rot spore', '#9a6aff', '#c8f070', 1.0),
    'tadpole-spirit': ('Pond spirit bolt', '#4ff0c8', '#e0fff6', 1.0),
    'violet-sporeling': ('Violet spore', '#b07aff', '#efe2ff', 1.0),
    'puffcap': ('Spore puff', '#ffe066', '#fff8d0', 0.7),
    'tree-frog': ('Water spit', '#7fd0e8', '#e8fbff', 0.35),
}
# Physical creatures keep their projectile and leave a wake.
PHYSICAL = {
    'needle-wasp': 'effect.weapons.thorn-flight',
    'thorn-slinger': 'effect.weapons.thorn-flight',
    'twigcaster': 'effect.weapons.thorn-flight',
    'seedcaster': 'effect.weapons.thorn-flight',
    'old-root': 'effect.weapons.thorn-flight',
    'spitter': 'effect.weapons.silk-flight',
}


def magic_bolt(name, title, colour, light, additive):
    """A glowing mote of the creature's colour with a streaming tail and motes shed behind it."""
    return effect(f'effect.neutral.{name}-bolt', title,
                  f'The {name.replace("-", " ")}\'s shot: a glowing mote with a streaming tail, shedding sparkles '
                  'that hang in the air.', FLIGHT, [
                      carrier(0.35, head=(light, 1.0, 2.4)),
                      layer('glow', 'missile', colour, WHITE, FLIGHT, 1, 2.4, 0, arcHeight=0.35, blend='additive',
                            intensity=0.9, texture=MOTE),
                      wake('tail', 'path', colour, colour, width=1.0, ticks=9, opacity=1.3, additive=min(additive, 0.65),
                           texture=FLAME_STRIP, tile=2.4, scroll=4.0),
                      motes('sparkles', 'path', light, colour, 45, 0.32, 18, additive=additive > 0.5, intensity=1.8,
                            core=0.6, speed=0.8, gravity=-0.4,
                            turbulence={'strength': 0.25, 'scale': 0.8, 'speed': 2.0}),
                  ])


def magic_hit(name, title, colour, light, additive):
    return effect(f'effect.neutral.{name}-burst', title + ' burst',
                  f'Where the {name.replace("-", " ")}\'s shot lands: a flash, a ring and a burst of motes.', 26, [
                      flare('flare', light, 3.0, duration=5, intensity=2.4),
                      ring('ring', colour, 3.6, duration=10, intensity=1.0 if additive > 0.5 else 0.7,
                           additive=additive > 0.5),
                      burst('motes', light, colour, 14, 0.3, speed=6.0, lifetime=18, gravity=3.0,
                            additive=additive > 0.5, intensity=1.6),
                  ])


def main():
    write('effects/weapons/thorn-flight.json', thorn_flight())
    write('effects/weapons/silk-flight.json', silk_flight())
    write('effects/weapons/splinter-hit.json', splinter_hit())
    for name, (title, colour, light, additive) in MAGIC.items():
        write(f'effects/neutral/{name}-bolt.json', magic_bolt(name, title, colour, light, additive))
        write(f'effects/neutral/{name}-burst.json', magic_hit(name, title, colour, light, additive))
    # Point each ranged creature at its flight and impact.
    for name in list(MAGIC) + list(PHYSICAL):
        rel = f'units/neutral/{name}.json'
        doc = json.load(open(os.path.join(BASE, rel)))
        pres = doc.setdefault('presentation', {})
        if name in MAGIC:
            pres.pop('projectile', None)
            pres['launchEffect'] = f'effect.neutral.{name}-bolt'
            pres['impactEffect'] = f'effect.neutral.{name}-burst'
        else:
            if name == 'spitter':
                pres['projectile'] = 'effect.weapon.silk'
            pres['launchEffect'] = PHYSICAL[name]
            pres['impactEffect'] = 'effect.weapons.splinter-hit'
        write(rel, doc)
    print('wrote the shared flights and', len(MAGIC) + len(PHYSICAL), 'forest creatures\' attacks')


if __name__ == '__main__':
    main()
