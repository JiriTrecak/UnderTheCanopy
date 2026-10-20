"""The ants' visual effects (D65, docs/ants.md): bio-magic, never corruption.

The colony's magic is a luminous violet LIQUID, its royalty gold, its pheromones a drifting
violet-blue scent, its fungus pale violet. Every spell has three moments, so it reads at the
game camera: a wind-up at the caster (`accepted`: rising motes and a small glyph), an impact
with heft (a flash of light, a shock ring, a splash, petals or earth), and a lingering mark
on whoever it changed (`statusApplied` for the status's whole life). Hero spells add a
pillar of light and a ground sigil the size of the fight; their ultimates are the biggest
things on the battlefield. Weapons hit with sprites, never lights (the eight effect lights
belong to spells and working buildings).

The art is the ants' own (asset.effects.ants.*, painted on black for additive blending),
plus the shared spark, glint and ring sprites. gen.py binds these effects to abilities and
weapons.
"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'beetles'))
from gen_common import (write, layer, emitter, flash, ring, glow, burst, pulse, effect, puffs, VIOLET, VIOLET_LIGHT,
                        WHITE, BURST_TEX, FLIPBOOK)
from gen_effects import streaks, glint, orb, shock, SPARK_DOT

S = 40

VIOLET_HOT = '#b49cff'
GOLD = '#ffe08a'
GOLD_LIGHT = '#fff0c0'
AMBER = '#ffc870'
SCENT = '#9f8cff'
SCENT_LIGHT = '#efe6ff'
ALARM = '#ffa850'
FUNGUS = '#c9b0ff'
FUNGUS_LIGHT = '#f6f0ff'
CRIMSON = '#ff4f6a'
CRIMSON_LIGHT = '#ffd0d8'
BLUE = '#5b7bff'
BLUE_LIGHT = '#c8d4ff'
EARTH = '#c8a070'
EARTH_LIGHT = '#f2dcb4'

FX = {}


def art(name):
    return {'asset': 'asset.effects.ants.' + name, 'role': 'image', 'index': 1}


SIGIL = art('royal-sigil')
SCENT_GLYPH = art('pheromone-sigil')
FUNGUS_RING = art('fungus-ring')
CREST = art('crest')
SPLASH = art('liquid-splash')
MIST = art('mist')
PETAL = art('petal')
SHELL = art('shell')
SLASH = art('slash')
EARTH_BURST = art('earth-burst')


def put(key, e):
    FX[key] = e['id']
    write('effects/ants/kit/' + e['id'].split('.', 3)[3] + '.json', e)
    return e['id']


def eff(key, name, description, duration, layers, loop=False):
    return put(key, effect('effect.ants.kit.' + key, name, description, duration, layers, loop=loop))


# ----- building blocks ----------------------------------------------------------------------

def sigil(id_, texture, colour, size, duration, height=0.08, spin=320, sustain=False, start=0, intensity=1.4,
          grow=True, **extra):
    """A glyph laid on the ground, turning slowly; it blooms out from the centre. The painted
    glyph fades towards its edges, so it is drawn larger than the area it marks."""
    size *= 1.6
    intensity *= 1.8
    motion = {'rotation': {'periodTicks': spin, 'direction': 'clockwise'}}
    if grow:
        motion['scale'] = {'periodTicks': max(2, min(duration, 24) * 2), 'min': 0.6, 'max': 1.0,
                           'easing': 'smoothstep'}
    if sustain:
        motion['opacity'] = pulse(80, 0.7, 1.0)
        motion.pop('scale', None)
    l = layer(id_, 'billboard', colour, WHITE, duration, 1, size, height, startTick=start, orientation='ground',
              blend='additive', intensity=intensity, core=0.3, texture=texture, motion=motion)
    if sustain:
        l['sustain'] = True
    l.update(extra)
    return l


def sprite(id_, texture, colour, size, duration, height=1.0, start=0, intensity=1.6, grow=(0.7, 1.0), fade=True,
           **extra):
    """A camera-facing painted sprite that blooms in and fades."""
    size *= 1.5
    intensity *= 1.6
    motion = {}
    if grow:
        motion['scale'] = {'periodTicks': duration * 2, 'min': grow[0], 'max': grow[1], 'easing': 'smoothstep'}
    if fade:
        motion['opacity'] = {'periodTicks': duration * 2, 'min': 0.0, 'max': 1.0, 'easing': 'sine', 'phase': 0.5}
    l = layer(id_, 'billboard', colour, WHITE, duration, 1, size, height, startTick=start, blend='additive',
              intensity=intensity, core=0.4, texture=texture)
    if motion:
        l['motion'] = motion
    l.update(extra)
    return l


def pillar(id_, colour, accent, size, height, duration, start=0, intensity=1.2, sustain=False):
    intensity *= 1.5
    l = layer(id_, 'pillar', colour, accent, duration, 1, size, height, startTick=start, blend='additive',
              intensity=intensity)
    if sustain:
        l['sustain'] = True
    return l


def drift(id_, texture, colour, accent, count, size, lifetime, spawn, rise=1.4, height=0.6, mode='burst',
          duration=None, sustain=False, start=0, intensity=1.2, speed=1.2, cone=40, spin=2.0, gravity=None):
    """Petals, spores or scent drifting up and out."""
    size *= 2.0
    intensity *= 1.8
    l = layer(id_, 'particles', colour, accent, duration or lifetime + 4, count, size, height, startTick=start,
              blend='additive', intensity=intensity, core=0.3, texture=texture,
              emitter=emitter(mode, lifetime, count, speed, variation=0.6, direction=(0, 1, 0), cone=cone,
                              gravity=-rise if gravity is None else gravity, drag=1.2, spawn=spawn, start=size,
                              end=size * 0.5, spin=spin, fade_in=0.15, fade_out=0.6))
    if sustain:
        l['sustain'] = True
    return l


def mist(id_, colour, count, size, lifetime, spawn, mode='burst', duration=None, sustain=False, start=0, rise=0.3,
         intensity=0.7, speed=0.8, height=0.5):
    """Billowing scent or spore cloud."""
    return drift(id_, MIST, colour, WHITE, count, size * 0.7, lifetime, spawn, rise=rise, height=height, mode=mode,
                 duration=duration, sustain=sustain, start=start, intensity=intensity, speed=speed, cone=80, spin=0.6)


def light(id_, colour, accent, reach, intensity, duration, height=1.5, start=0, sustain=False, pulse_=None):
    l = flash(id_, colour, accent, reach, intensity, duration=duration, height=height, start=start)
    if sustain:
        l['sustain'] = True
    if pulse_:
        l['motion'] = {'opacity': pulse_}
    return l


def aura(key, name, colour, accent, size=1.6, flame=False, texture=None, light_=False):
    """Worn while a status lasts: a ring at the feet, rising motes (or flames), an optional sigil."""
    layers = [ring('ring', colour, accent, size, 80, sustain=True, follow=True, intensity=0.55,
                   motion={'opacity': pulse(40, 0.55, 1.0)}),
              glow('pool', colour, accent, size * 0.9, 80, height=0.08, sustain=True, follow=True, intensity=0.3)]
    if flame:
        layers.append(streaks('flames', colour, accent, count=10, speed=2.6, size=0.1, lifetime=18, gravity=-5.0,
                              cone=18, spawn=size * 0.35, mode='continuous', duration=80, sustain=True,
                              stretch=0.12, intensity=3.0, follow=True))
    else:
        layers.append(drift('motes', PETAL, colour, accent, 4, 0.22, 44, size * 0.4, mode='continuous', duration=80,
                            sustain=True, intensity=1.0))
    if texture:
        layers.append(sigil('sigil', texture, colour, size * 2.2, 80, sustain=True, grow=False, follow=True,
                            intensity=1.0))
    if light_:
        layers.append(light('light', colour, accent, size * 3, 1.2, 80, height=1.0, sustain=True,
                            pulse_=pulse(40, 0.6, 1.0)))
    return eff(key, name, name + ': worn for as long as it lasts.', 80, layers, loop=True)


def windup(key, name, colour, accent, size=2.0, duration=30):
    """A caster gathering its magic: a small glyph, rising motes and a glow in the hands."""
    return eff(key, name, name + ': the caster gathers its magic.', duration, [
        sigil('glyph', SCENT_GLYPH, colour, size * 1.8, duration, intensity=1.0),
        drift('motes', PETAL, colour, accent, 10, 0.24, duration, size * 0.5, rise=2.4, height=0.4),
        orb('hands', colour, accent, size * 0.6, duration, height=2.2, intensity=1.2),
    ])


# ----- effects ------------------------------------------------------------------------------

def build():
    # Shared wind-ups.
    windup('cast-scent', 'Scent wind-up', SCENT, SCENT_LIGHT)
    windup('cast-royal', 'Royal wind-up', GOLD, GOLD_LIGHT, size=2.6)
    windup('cast-substance', 'Substance wind-up', VIOLET, VIOLET_LIGHT, size=2.4)
    windup('cast-war', 'War wind-up', AMBER, GOLD_LIGHT, size=2.4)

    # --- Weapons: every ant blow lands with weight (sprites, no lights).
    eff('hit-bite', 'Bite', 'A snap of mandibles: an amber slash and a puff of dust.', 20, [
        sprite('slash', SLASH, AMBER, 1.8, 10, height=0.7, grow=(0.7, 1.0)),
        streaks('chips', AMBER, GOLD_LIGHT, count=6, speed=6, size=0.06, lifetime=10, gravity=16, cone=80, height=0.5),
        puffs('dust', EARTH, EARTH_LIGHT, count=2, speed=1.0, size=0.5, height=0.0, lifetime=16, rise=-0.5),
    ])
    eff('hit-needle', 'Needle hit', 'A chitin needle strikes: a violet star and splinters.', 20, [
        glint('glint', VIOLET, 1.6, duration=5, height=0.4, intensity=2.6),
        streaks('splinters', VIOLET_HOT, VIOLET_LIGHT, count=8, speed=7, size=0.06, lifetime=10, gravity=12,
                cone=70, height=0.4),
    ])
    eff('hit-heavy', 'Heavy blow', 'A heavy head-butt or slam: a shock ring, a dust burst and a glint.', 28, [
        glint('glint', GOLD, 2.2, duration=5, height=0.7, intensity=2.4),
        shock('shock', GOLD, 4.0, 14, intensity=1.4),
        puffs('dust', EARTH, EARTH_LIGHT, count=6, speed=2.4, size=0.8, height=0.0, lifetime=22, rise=-0.4, cone=85),
    ])
    eff('hit-sting', 'Sting', 'A violet sting: a bright glint and venom droplets.', 22, [
        glint('glint', VIOLET_HOT, 1.8, duration=5, height=0.6, intensity=3.0),
        streaks('venom', VIOLET, VIOLET_LIGHT, count=8, speed=5, size=0.07, lifetime=12, gravity=14, cone=60,
                height=0.6),
    ])
    eff('hit-reaver', 'Reaver blow', 'The Reaver Queen\'s blades: a great slash, shock and dust.', 30, [
        sprite('slash', SLASH, AMBER, 3.6, 12, height=1.0, grow=(0.7, 1.1)),
        shock('shock', AMBER, 6.0, 16, intensity=1.6),
        puffs('dust', EARTH, EARTH_LIGHT, count=8, speed=3.0, size=1.0, height=0.0, lifetime=26, rise=-0.4, cone=85),
    ])
    eff('glob-impact', 'Nectar glob', 'A glob of glowing nectar bursts: an amber splash, droplets and a sticky pool.',
        60, [
            sprite('splash', SPLASH, AMBER, 4.2, 16, height=0.8, grow=(0.7, 1.1)),
            shock('shock', AMBER, 6.0, 16, intensity=1.4),
            layer('drops', 'rain', AMBER, GOLD_LIGHT, 14, 12, 0.2, 2.5, spread=3.0, fallSpeed=1.6,
                  launchDelayMs=120, length=2, blend='additive', intensity=2.2, core=0.8, texture=SPARK_DOT,
                  impact={'durationTicks': 10, 'count': 3, 'size': 0.14, 'spread': 0.6, 'height': 0.2,
                          'colour': AMBER, 'accent': GOLD_LIGHT}),
            glow('pool', AMBER, GOLD_LIGHT, 3.0, 60, height=0.06, intensity=0.5,
                 motion={'opacity': pulse(120, 0.0, 1.0, 'sine')}),
        ])
    eff('spit-muzzle', 'Spit', 'A spit of violet venom from the spitter\'s mouth.', 16, [
        glint('flash', VIOLET_HOT, 1.4, duration=4, anchor='source', intensity=2.4),
        streaks('spray', VIOLET, VIOLET_LIGHT, count=5, speed=6, size=0.05, lifetime=8, gravity=6, cone=30,
                anchor='source'),
    ])

    # --- Raider, Needler, Burster, Major, Bullet Ant.
    aura('frenzy', 'Frenzy', CRIMSON, CRIMSON_LIGHT, 1.2, flame=True)
    eff('barbed', 'Barbed', 'A barbed needle lodged in the armour: a small violet star that throbs.', 80, [
        glint('star', VIOLET, 1.0, duration=80, height=1.4, intensity=1.6, sustain=True, follow=True,
              motion={'opacity': pulse(30, 0.4, 1.0)}),
    ], loop=True)
    eff('burst', 'Substance burst',
        'The sac ruptures: a blinding violet flash, a crown of liquid light, droplets raining down, a shock ring and a glowing pool that fades.',
        90, [
            light('light', VIOLET, VIOLET_LIGHT, 14, 6.0, 16, height=1.5),
            sprite('splash', SPLASH, VIOLET_HOT, 10.0, 20, height=2.0, grow=(0.7, 1.2), intensity=2.4),
            layer('blast', 'billboard', WHITE, WHITE, 32, 1, 6.0, 1.4, blend='additive', flipbook=FLIPBOOK,
                  texture=BURST_TEX, intensity=1.2),
            shock('shock', VIOLET_HOT, 9.0, 16, intensity=2.2),
            shock('shock2', VIOLET, 6.0, 22, start=4),
            layer('drops', 'rain', VIOLET, VIOLET_LIGHT, 18, 20, 0.22, 4.0, spread=4.0, fallSpeed=1.4,
                  launchDelayMs=150, length=2.5, blend='additive', intensity=2.6, core=0.9, texture=SPARK_DOT,
                  impact={'durationTicks': 12, 'count': 4, 'size': 0.16, 'spread': 0.7, 'height': 0.2,
                          'colour': VIOLET, 'accent': VIOLET_LIGHT}),
            streaks('spray', VIOLET_HOT, VIOLET_LIGHT, count=36, speed=12, size=0.1, lifetime=20, gravity=18, cone=75,
                    height=0.6, intensity=4.0),
            mist('mist', VIOLET, 8, 1.6, 50, 1.6, start=2),
            glow('pool', VIOLET, VIOLET_LIGHT, 4.0, 90, height=0.06, intensity=0.6,
                 motion={'opacity': pulse(180, 0.0, 1.0, 'sine')}),
        ])
    aura('guard', 'Guard', GOLD, GOLD_LIGHT, 2.6, texture=CREST)
    aura('guarded', 'Guarded', GOLD, GOLD_LIGHT, 1.0)
    eff('plug', 'Plug', 'The Major plants its head: a dome of amber chitin over it, a ring on the ground.', 80, [
        sprite('dome', SHELL, AMBER, 4.6, 80, height=1.6, grow=None, fade=False, sustain=True, follow=True,
               intensity=1.0, motion={'opacity': pulse(60, 0.5, 0.85)}),
        sigil('crest', CREST, GOLD, 5.5, 80, sustain=True, grow=False, follow=True, intensity=1.2),
        ring('ring', GOLD, GOLD_LIGHT, 3.0, 80, sustain=True, follow=True, intensity=0.7),
    ], loop=True)
    eff('plug-slam', 'Plug slam', 'The Major slams its head down: shock and dust.', 24, [
        shock('shock', GOLD, 7.0, 16, intensity=1.8),
        puffs('dust', EARTH, EARTH_LIGHT, count=8, speed=2.8, size=1.0, height=0.0, lifetime=22, rise=-0.4, cone=85),
        glint('glint', GOLD, 3.0, duration=6, height=1.2),
    ])
    eff('agony-hit', 'Agony strike', 'The sting lands: a violet starburst, a shock and a flash of pain.', 30, [
        light('light', VIOLET_HOT, WHITE, 10, 4.5, 12, height=1.2),
        glint('star', VIOLET_HOT, 6.0, duration=10, height=1.2, intensity=3.4),
        sprite('slash', SLASH, VIOLET_HOT, 5.0, 12, height=1.2, intensity=2.0),
        shock('shock', CRIMSON, 8.0, 16, intensity=2.0),
        streaks('venom', VIOLET, VIOLET_LIGHT, count=18, speed=8, size=0.08, lifetime=14, gravity=10, cone=100,
                height=0.8),
    ])
    aura('agony', 'Agony', CRIMSON, VIOLET_LIGHT, 1.4, flame=True)

    # --- Herald.
    eff('trail', 'Scent trail', 'A pheromone trail: a turning scent glyph on the ground and drifting violet scent.',
        12 * S, [
            sigil('glyph', SCENT_GLYPH, SCENT, 11.0, 12 * S, sustain=True, grow=False, intensity=1.3, spin=480),
            mist('scent', SCENT, 4, 1.6, 60, 4.0, mode='continuous', duration=12 * S, sustain=True, intensity=0.55),
            drift('motes', PETAL, SCENT, SCENT_LIGHT, 4, 0.22, 50, 4.5, mode='continuous', duration=12 * S,
                  sustain=True, rise=2.0),
        ])
    eff('trail-lay', 'Trail laid', 'The trail blooms where it is laid.', 30, [
        shock('shock', SCENT, 10.0, 18, intensity=1.4),
        mist('puff', SCENT, 6, 1.8, 30, 3.0),
    ])
    aura('trailed', 'On the trail', SCENT, SCENT_LIGHT, 1.0)
    eff('alarm', 'Alarm',
        'The Herald sounds the alarm: a blaze of amber light, two waves of scent racing out, a column of fire-coloured light.',
        44, [
            light('light', ALARM, GOLD_LIGHT, 18, 5.0, 18, height=2.0),
            pillar('column', ALARM, GOLD_LIGHT, 2.4, 9.0, 30),
            shock('wave', ALARM, 18.0, 22, intensity=2.2),
            shock('wave2', GOLD, 13.0, 26, start=6, intensity=1.6),
            sigil('glyph', SCENT_GLYPH, ALARM, 12.0, 40, intensity=1.4),
            streaks('embers', ALARM, GOLD_LIGHT, count=30, speed=7, size=0.1, lifetime=24, gravity=-6, cone=40,
                    spawn=2.0, height=0.5, intensity=3.5),
        ])
    aura('alarmed', 'Alarmed', ALARM, GOLD_LIGHT, 1.2, flame=True)
    eff('panic', 'Panic', 'A cloud of fear-scent rolls over the enemy.', 3 * S, [
        sigil('glyph', SCENT_GLYPH, VIOLET, 11.0, 3 * S, intensity=1.2),
        mist('cloud', VIOLET, 18, 2.6, 70, 4.0, start=0, intensity=0.8),
        shock('shock', VIOLET_HOT, 11.0, 16, intensity=1.6),
        light('light', VIOLET, VIOLET_LIGHT, 12, 3.0, 20, height=1.5),
    ])
    aura('panicked', 'Panicked', VIOLET, SCENT_LIGHT, 1.0)

    # --- Gardener.
    eff('fungus', 'Fungus circle', 'A fairy ring of glowing fungus grows and breathes healing light.', 10 * S, [
        sigil('ring', FUNGUS_RING, FUNGUS, 11.0, 10 * S, sustain=True, grow=False, intensity=1.5, spin=960),
        glow('light', FUNGUS, FUNGUS_LIGHT, 8.0, 10 * S, height=0.1, sustain=True, intensity=0.35),
        drift('spores', PETAL, FUNGUS, FUNGUS_LIGHT, 6, 0.2, 60, 4.5, mode='continuous', duration=10 * S,
              sustain=True, rise=1.6),
        light('glow', FUNGUS, FUNGUS_LIGHT, 9, 1.6, 10 * S, height=1.0, sustain=True, pulse_=pulse(80, 0.5, 1.0)),
    ])
    eff('fungus-grow', 'Fungus grows', 'The ring bursts out of the soil.', 30, [
        shock('shock', FUNGUS, 11.0, 18, intensity=1.6),
        drift('spores', PETAL, FUNGUS, FUNGUS_LIGHT, 20, 0.26, 30, 4.0, rise=3.0),
    ])
    eff('fungus-heal', 'Fungus healing', 'Healing spores rise around a unit.', 30, [
        drift('motes', PETAL, FUNGUS, FUNGUS_LIGHT, 8, 0.24, 30, 0.8, rise=3.0, height=0.3),
        pillar('column', FUNGUS, FUNGUS_LIGHT, 0.8, 3.0, 24, intensity=0.8),
    ])
    eff('spores', 'Spore cloud', 'A choking cloud of violet spores hangs over the ground.', 8 * S, [
        mist('cloud', FUNGUS, 6, 2.8, 70, 4.5, mode='continuous', duration=8 * S, sustain=True, intensity=0.7),
        drift('spores', PETAL, FUNGUS, FUNGUS_LIGHT, 6, 0.18, 60, 4.5, mode='continuous', duration=8 * S,
              sustain=True, rise=0.6),
        sigil('ring', FUNGUS_RING, FUNGUS, 10.0, 8 * S, sustain=True, grow=False, intensity=0.8, spin=960),
    ])
    aura('choked', 'Choking spores', FUNGUS, FUNGUS_LIGHT, 1.0)

    # --- Reaver Queen.
    eff('devour', 'Devour', 'Huge mandibles close: a great slash, a gulp of violet light and mist.', 36, [
        sprite('slash', SLASH, VIOLET_HOT, 7.0, 14, height=1.6, grow=(0.7, 1.2), intensity=2.2),
        light('light', VIOLET, VIOLET_LIGHT, 12, 4.0, 14, height=1.2),
        mist('gulp', VIOLET, 8, 1.4, 34, 1.2),
        shock('shock', VIOLET, 5.0, 14, intensity=1.4),
    ])
    eff('digest', 'Digestion', 'A pulse of violet light from the Reaver Queen\'s belly.', 16, [
        glow('pulse', VIOLET, VIOLET_LIGHT, 2.0, 16, height=1.0, orientation='camera', intensity=0.8),
    ])
    eff('rampage-run', 'Rampage', 'The Reaver Queen charges, throwing dust behind her.', 80, [
        puffs('dust', EARTH, EARTH_LIGHT, count=10, speed=1.6, size=1.2, height=0.0, lifetime=30, rise=-0.3,
              mode='continuous', spawn=1.0).__or__({'follow': True, 'anchor': 'source', 'durationTicks': 80}),
    ])
    eff('rampage', 'Rampage impact', 'She crashes into the enemy: earth bursts, a shock ring, a great slash.', 40, [
        sprite('earth', EARTH_BURST, EARTH_LIGHT, 11.0, 22, height=1.8, grow=(0.7, 1.1), intensity=2.0),
        sprite('slash', SLASH, AMBER, 8.0, 14, height=1.8, intensity=2.2),
        shock('shock', AMBER, 10.0, 18, intensity=2.0),
        puffs('dust', EARTH, EARTH_LIGHT, count=10, speed=4.0, size=1.2, height=0.0, lifetime=28, rise=-0.4, cone=85),
        light('light', AMBER, GOLD_LIGHT, 10, 3.0, 12, height=1.2),
    ])
    aura('presence', 'Royal presence', BLUE, BLUE_LIGHT, 1.0)
    aura('presence-source', 'Royal presence', BLUE, BLUE_LIGHT, 3.0, texture=SIGIL)
    eff('molt', 'Molt', 'The old carapace bursts open: a shell of light shatters, blue flash, shards.', 40, [
        sprite('shell', SHELL, BLUE, 6.0, 20, height=1.6, grow=(1.0, 1.5), intensity=1.6),
        light('light', BLUE, BLUE_LIGHT, 12, 4.0, 16, height=1.5),
        streaks('shards', BLUE_LIGHT, WHITE, count=24, speed=9, size=0.12, lifetime=20, gravity=14, cone=100,
                height=1.2, intensity=3.0),
        shock('shock', BLUE, 8.0, 16, intensity=1.6),
    ])
    aura('molting', 'Fresh carapace', BLUE, BLUE_LIGHT, 2.4)

    # --- Founding (Alate, Queen) and the tunnels.
    eff('found', 'Founding', 'A new mound bursts up out of the earth: a spray of soil, a golden flash, rising petals.',
        50, [
            sprite('earth', EARTH_BURST, EARTH_LIGHT, 16.0, 26, height=2.4, grow=(0.7, 1.2), intensity=2.0),
            light('light', GOLD, GOLD_LIGHT, 14, 4.0, 18, height=2.0),
            shock('shock', GOLD, 12.0, 20, intensity=1.8),
            puffs('dust', EARTH, EARTH_LIGHT, count=14, speed=4.5, size=1.6, height=0.0, lifetime=36, rise=-0.5,
                  cone=85, spawn=2.0),
            drift('petals', PETAL, GOLD, GOLD_LIGHT, 18, 0.3, 46, 3.0, rise=2.6, height=1.0),
        ])

    # --- The Queen.
    eff('jelly-beam', 'Royal jelly', 'A ribbon of golden light from the Queen to the one she feeds.', 18, [
        layer('beam', 'beam', GOLD, GOLD_LIGHT, 18, 1, 0.9, 1.6, blend='additive', follow=True, intensity=2.0),
        layer('core', 'beam', WHITE, WHITE, 14, 1, 0.25, 1.6, blend='additive', follow=True),
    ])
    eff('jelly', 'Royal jelly',
        'Golden light pours down on the unit: a pillar, a flash, petals and a ring of gold.', 50, [
            pillar('pillar', GOLD, GOLD_LIGHT, 2.8, 14.0, 34, intensity=1.6),
            light('light', GOLD, GOLD_LIGHT, 14, 5.0, 20, height=2.0),
            sigil('sigil', SIGIL, GOLD, 9.0, 40, intensity=1.6),
            shock('shock', GOLD, 10.0, 18, intensity=2.0),
            drift('petals', PETAL, GOLD, GOLD_LIGHT, 24, 0.3, 40, 2.0, rise=3.0, height=0.4),
            glint('star', GOLD_LIGHT, 5.0, duration=10, height=3.0, intensity=3.0),
        ])
    aura('jellied', 'Royal jelly', GOLD, GOLD_LIGHT, 1.2, flame=True)
    aura('one-body-source', 'One body', SCENT, SCENT_LIGHT, 4.0, texture=SIGIL)
    aura('one-body', 'One body', SCENT, SCENT_LIGHT, 1.0)
    eff('exodus-gather', 'Exodus',
        'The colony gathers to go under: a great royal sigil opens around the Queen and a column of violet light rises.',
        80, [
            sigil('sigil', SIGIL, VIOLET_HOT, 22.0, 80, intensity=1.6, spin=200),
            pillar('pillar', VIOLET, VIOLET_LIGHT, 3.0, 14.0, 80, intensity=1.4),
            light('light', VIOLET, VIOLET_LIGHT, 20, 4.0, 80, height=3.0, pulse_=pulse(20, 0.5, 1.0)),
            drift('motes', PETAL, VIOLET, VIOLET_LIGHT, 40, 0.3, 60, 8.0, rise=3.0, height=0.3),
            mist('earth', EARTH, 10, 2.4, 60, 8.0, start=30, intensity=0.5),
        ])
    eff('exodus-rise', 'Exodus arrival', 'The colony bursts up out of the ground.', 40, [
        sprite('earth', EARTH_BURST, EARTH, 4.0, 18, height=0.8, grow=(0.7, 1.1)),
        drift('motes', PETAL, VIOLET, VIOLET_LIGHT, 8, 0.24, 30, 1.0, rise=3.0),
        puffs('dust', EARTH, EARTH_LIGHT, count=4, speed=2.0, size=0.9, height=0.0, lifetime=24, rise=-0.4, cone=85),
    ])

    # --- The Royal Alchemist.
    eff('draught-flight', 'Volatile draught', 'A flask of glowing violet liquid arcs through the air.', 40, [
        layer('flask', 'missile', VIOLET_HOT, VIOLET_LIGHT, 40, 1, 0.6, 0, arcHeight=3.0, blend='additive',
              texture=SPARK_DOT, intensity=4.0, core=1.0,
              trail={'durationTicks': 10, 'maxPoints': 16, 'width': 0.25, 'colour': VIOLET, 'opacity': 0.9,
                     'breakDistance': 6}),
    ])
    eff('draught', 'Volatile draught',
        'The flask shatters: a crown of violet liquid, droplets raining over the area, a flash and a shock ring.', 60, [
            light('light', VIOLET, VIOLET_LIGHT, 14, 5.0, 16, height=1.5),
            sprite('splash', SPLASH, VIOLET_HOT, 11.0, 20, height=1.8, grow=(0.7, 1.2), intensity=2.4),
            shock('shock', VIOLET_HOT, 10.0, 18, intensity=2.0),
            layer('drops', 'rain', VIOLET, VIOLET_LIGHT, 18, 22, 0.22, 4.0, spread=4.0, fallSpeed=1.4,
                  launchDelayMs=150, length=2.5, blend='additive', intensity=2.6, core=0.9, texture=SPARK_DOT,
                  impact={'durationTicks': 12, 'count': 4, 'size': 0.16, 'spread': 0.7, 'height': 0.2,
                          'colour': VIOLET, 'accent': VIOLET_LIGHT}),
            mist('mist', VIOLET, 10, 1.8, 50, 3.0, start=2),
            glow('pool', VIOLET, VIOLET_LIGHT, 4.5, 60, height=0.06, intensity=0.6,
                 motion={'opacity': pulse(120, 0.0, 1.0, 'sine')}),
        ])
    eff('draught-hit', 'Burnt by venom', 'Violet droplets sizzle on a unit.', 20, [
        streaks('sizzle', VIOLET, VIOLET_LIGHT, count=6, speed=3, size=0.07, lifetime=14, gravity=-3, cone=30,
                height=0.8),
    ])
    eff('transfusion-draw', 'Transfusion', 'A crimson stream drawn out of the enemy towards the Alchemist.', 24, [
        layer('beam', 'beam', CRIMSON, CRIMSON_LIGHT, 24, 1, 0.9, 1.4, blend='additive', follow=True, intensity=2.0),
        layer('core', 'beam', WHITE, WHITE, 20, 1, 0.25, 1.4, blend='additive', follow=True),
        streaks('blood', CRIMSON, CRIMSON_LIGHT, count=12, speed=4, size=0.08, lifetime=16, gravity=-2, cone=50,
                height=1.0),
    ])
    eff('transfusion-pour', 'Transfusion', 'Life pours into the wounded ally: a crimson-violet pillar and motes.', 40, [
        pillar('pillar', CRIMSON, VIOLET_LIGHT, 2.2, 11.0, 30, intensity=1.5),
        sigil('sigil', SCENT_GLYPH, CRIMSON, 6.0, 30, intensity=1.4),
        drift('motes', PETAL, CRIMSON, CRIMSON_LIGHT, 18, 0.26, 34, 1.6, rise=3.0, height=0.3),
        light('light', CRIMSON, VIOLET_LIGHT, 10, 3.5, 18, height=1.5),
    ])
    eff('crystal', 'Crystallised', 'Encased in a glassy shell of hardened substance.', 80, [
        sprite('shell', SHELL, VIOLET_HOT, 6.0, 80, height=2.0, grow=None, fade=False, sustain=True, follow=True,
               intensity=1.4, motion={'opacity': pulse(50, 0.7, 1.0)}),
        glow('base', VIOLET, VIOLET_LIGHT, 2.6, 80, height=0.06, sustain=True, follow=True, intensity=0.6),
        light('light', VIOLET, VIOLET_LIGHT, 6, 1.6, 80, height=1.4, sustain=True),
    ], loop=True)
    eff('crystal-form', 'Crystallise', 'The shell snaps shut: a flash and shards of light.', 24, [
        glint('star', VIOLET_LIGHT, 4.0, duration=8, height=1.4, intensity=3.2),
        streaks('shards', VIOLET_LIGHT, WHITE, count=16, speed=7, size=0.1, lifetime=14, gravity=10, cone=120,
                height=1.2),
        shock('shock', VIOLET, 5.0, 14),
    ])
    eff('overdose', 'Overdose',
        'The Alchemist pours out her substance: a vast violet sigil, a column of light, a wave and a rain of liquid light.',
        60, [
            light('light', VIOLET, VIOLET_LIGHT, 22, 6.0, 30, height=3.0),
            sigil('sigil', SIGIL, VIOLET_HOT, 16.0, 60, intensity=1.8, spin=160),
            pillar('pillar', VIOLET, VIOLET_LIGHT, 3.4, 16.0, 46, intensity=1.6),
            shock('wave', VIOLET_HOT, 16.0, 22, intensity=2.4),
            layer('rain', 'rain', VIOLET, VIOLET_LIGHT, 30, 40, 0.24, 9.0, spread=6.0, fallSpeed=1.2,
                  launchDelayMs=400, length=3.0, blend='additive', intensity=2.6, core=0.9, texture=SPARK_DOT,
                  impact={'durationTicks': 12, 'count': 3, 'size': 0.16, 'spread': 0.6, 'height': 0.2,
                          'colour': VIOLET, 'accent': VIOLET_LIGHT}),
        ])
    aura('overdosed', 'Overdosed', VIOLET, VIOLET_LIGHT, 1.4, flame=True)

    # --- The General.
    eff('charge-trail', 'Mandible charge', 'Dust and an amber streak behind the charging General.', 80, [
        puffs('dust', EARTH, EARTH_LIGHT, count=8, speed=1.4, size=1.0, height=0.0, lifetime=26, rise=-0.3,
              mode='continuous', spawn=0.8).__or__({'follow': True, 'anchor': 'source', 'durationTicks': 80}),
    ])
    eff('charge-hit', 'Mandible charge', 'The General seizes its prey: crossed slashes, a shock ring, a gold flash.',
        34, [
            sprite('slash', SLASH, GOLD, 7.0, 14, height=1.6, grow=(0.7, 1.2), intensity=2.2),
            sprite('slash2', SLASH, AMBER, 6.0, 14, height=1.4, start=3, grow=(0.7, 1.2), intensity=2.0,
                   motion={'rotation': {'periodTicks': 400, 'direction': 'counterclockwise', 'phaseDegrees': 90}}),
            shock('shock', GOLD, 11.0, 18, intensity=2.2),
            light('light', GOLD, GOLD_LIGHT, 12, 4.0, 12, height=1.5),
            puffs('dust', EARTH, EARTH_LIGHT, count=6, speed=3.0, size=0.9, height=0.0, lifetime=22, rise=-0.4,
                  cone=85),
        ])
    eff('marked', 'Marked prey',
        'A crimson hunting-scent glyph turns under the marked enemy and a mark of light hangs above it.', 80, [
            sigil('glyph', SCENT_GLYPH, CRIMSON, 4.2, 80, sustain=True, grow=False, follow=True, intensity=1.4,
                  spin=160),
            glint('mark', CRIMSON, 1.8, duration=80, height=3.6, intensity=2.2, sustain=True, follow=True,
                  motion={'opacity': pulse(30, 0.5, 1.0)}),
        ], loop=True)
    eff('mark-hit', 'Mark prey', 'The scent mark is laid: a crimson flash.', 20, [
        shock('shock', CRIMSON, 4.0, 14, intensity=1.6),
        glint('star', CRIMSON, 2.6, duration=6, height=1.6, intensity=2.6),
    ])
    aura('column', 'War column', BLUE, BLUE_LIGHT, 1.0)
    aura('column-source', 'War column', BLUE, BLUE_LIGHT, 3.4, texture=CREST)
    eff('bivouac', 'Bivouac',
        'The army locks into a living fortress: a great crest on the ground, a column of amber light, a shock and dust.',
        60, [
            light('light', AMBER, GOLD_LIGHT, 20, 5.0, 30, height=3.0),
            sigil('crest', CREST, GOLD, 16.0, 60, intensity=1.8, spin=640),
            pillar('pillar', AMBER, GOLD_LIGHT, 3.0, 12.0, 40, intensity=1.4),
            shock('shock', GOLD, 15.0, 20, intensity=2.2),
            puffs('dust', EARTH, EARTH_LIGHT, count=16, speed=4.0, size=1.4, height=0.0, lifetime=34, rise=-0.4,
                  cone=85, spawn=4.0),
        ])
    eff('bivouacked', 'Bivouac', 'A dome of amber chitin over each ant in the fortress.', 80, [
        sprite('dome', SHELL, AMBER, 3.4, 80, height=1.2, grow=None, fade=False, sustain=True, follow=True,
               intensity=1.0, motion={'opacity': pulse(60, 0.55, 0.9)}),
        ring('ring', GOLD, GOLD_LIGHT, 2.0, 80, sustain=True, follow=True, intensity=0.6),
    ], loop=True)
    return FX
