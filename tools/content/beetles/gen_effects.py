"""The beetles' visual effects: substance blue-violet, fire orange, steam white (D65).

Substance glows blue to violet and only ever inside or out of a machine: coils, canisters,
bolt heads, the furnace. Combat reads at the normal game camera through crisp parts: hot
streaks along their velocity (`stretch`), star glints and ragged muzzle flashes, thin shock
rings, glass shards, all additive with white-hot cores (`core`), never soft blobs. The art
is `gen_art.py`'s (asset.effects.beetles.*). Lights are kept for spells and heavy guns: of
the eight effect lights, five belong to working machines and buildings, so fast weapons
flash with sprites instead.

Also writes the machines' weapon effects (effects/units/) and the Hornwatch's shot and the
Brood Den's pressure (effects/buildings/), so every beetle weapon reads alike.
"""
from gen_common import *
import os as _os
import sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.dirname(_os.path.abspath(__file__))))
import vfx_layers as vl  # noqa: E402  (tools/content/vfx_layers.py)

E = {}


def add(rel, e):
    E[e['id']] = e
    write('effects/beetles/' + rel + '.json', e)
    return e['id']


def add_at(rel, e):
    """An effect outside effects/beetles (machines and buildings keep their ids)."""
    E[e['id']] = e
    write('effects/' + rel + '.json', e)
    return e['id']


def art(name):
    return {'asset': 'asset.effects.beetles.' + name, 'role': 'image', 'index': 1}


SPARK_DOT = art('spark')
GLINT = art('glint')
FLASH_ART = art('flash')
SHARD_ART = art('shard')
RING_ART = art('ring')
MOLTEN = art('molten')
SCORCH = art('scorch')

PUFF = {'asset': 'asset.effects.vfx.puff', 'role': 'image', 'index': 1}
FLAME_STRIP = {'asset': 'asset.effects.vfx.flame-strip', 'role': 'image', 'index': 1}
ASH = {'asset': 'asset.effects.vfx.ash', 'role': 'image', 'index': 1}
CHUNK = {'asset': 'asset.effects.vfx.chunk', 'role': 'image', 'index': 1}
BLAST_RING = {'asset': 'asset.effects.vfx.blast-ring', 'role': 'image', 'index': 1}
# Four still variants of a 2 × 2 atlas, one picked per particle.
VARIANTS = {'columns': 2, 'rows': 2, 'firstFrame': 0, 'frames': 4, 'frameTicks': 400, 'mode': 'once',
            'randomStart': True}
# Texture brightness → fire colour: a dark ember rim, red, orange, a white-hot heart.
FIRE_RAMP = [{'at': 0.0, 'colour': '#3a1206'}, {'at': 0.25, 'colour': '#b8320a'}, {'at': 0.5, 'colour': '#ff7a1a'},
             {'at': 0.75, 'colour': '#ffc04a'}, {'at': 1.0, 'colour': '#fff4d0'}]


def keys(*pairs):
    """A curve over a life: (at, value) pairs."""
    return [{'at': at, 'value': value} for at, value in pairs]


def fire_trail(id_, rides, start, duration):
    """Fire left hanging in the air behind a falling or flying layer: hot puffs born where the
    layer passes, swelling, cooling down the fire ramp and turning into dark smoke."""
    return layer(id_, 'particles', WHITE, WHITE, duration, 110, 1.0, 0, startTick=start, blend='additive',
                 rides=rides, offset={'x': 0, 'y': 1.6, 'z': 0}, texture=PUFF, flipbook=VARIANTS, ramp=FIRE_RAMP,
                 emitter=emitter('continuous', 24, 180, 2.4, variation=0.6, cone=180, drag=2.0, spawn=1.3,
                                 start=1.4, end=3.8) | {
                     'space': 'world', 'inheritVelocity': 0.1,
                     'sizeOverLife': keys((0, 1.4), (0.2, 2.6), (1, 4.0)),
                     'colourOverLife': [{'at': 0, 'colour': WHITE}, {'at': 0.45, 'colour': WHITE},
                                        {'at': 0.8, 'colour': '#6a5448'}, {'at': 1, 'colour': '#3a3532'}],
                     'additiveOverLife': keys((0, 0.65), (0.45, 0.6), (0.75, 0)),
                     'opacityOverLife': keys((0, 0), (0.08, 1), (0.6, 0.8), (1, 0)),
                     'erosionOverLife': keys((0, 0), (0.5, 0.15), (1, 0.75)),
                     'heatOverLife': keys((0, 1.2), (1, 0.4)),
                     'turbulence': {'strength': 0.4, 'scale': 1.5, 'speed': 1.5}})


SUBSTANCE = '#8f66ff'  # violet sparks
SUBSTANCE_HOT = '#b49cff'
EMBER = '#ffa040'


def missile_model(asset, size=1.0, arc=0.0, index=1):
    return layer('body', 'missile', WHITE, WHITE, 40, 1, size, 0, arcHeight=arc,
                 model={'asset': asset, 'role': 'geometry', 'index': index, 'rotation': {'x': 0, 'y': 0, 'z': 0}})


def tracer(colour, accent, size=0.3, length=10.0, width=0.18, trail=6, trail_colour=None, intensity=8.0, arc=None):
    """A bright elongated head along the flight with a thin fading ribbon behind it."""
    l = layer('tracer', 'missile', colour, accent, 40, 1, size, 0, blend='additive', texture=SPARK_DOT,
              intensity=intensity, core=1.0, length=length,
              trail={'durationTicks': trail, 'maxPoints': 12, 'width': width, 'colour': trail_colour or colour,
                     'opacity': 1.0, 'breakDistance': 6})
    if arc is not None:
        l['arcHeight'] = arc
    return l


def head(colour, accent, size=0.3, length=10.0, intensity=8.0, arc=None):
    """A bright elongated head along the flight (the path the shot's trails ride)."""
    l = layer('tracer', 'missile', colour, accent, 40, 1, size, 0, blend='additive', texture=SPARK_DOT,
              intensity=intensity, core=1.0, length=length)
    if arc is not None:
        l['arcHeight'] = arc
    return l


def substance_trail(colour, light, width=0.6, rate=70, size=0.14):
    """A violet light wake and hot sparks shed behind a bolt, hanging in the air."""
    return [vl.wake('trail-wake', 'tracer', light, colour, width=width, ticks=6, opacity=1.2, additive=0.7,
                    texture=vl.WAKE, tile=2.0, scroll=2.0),
            vl.motes('trail-sparks', 'tracer', light, colour, rate, size, 9, intensity=3.0, core=0.8, speed=1.4,
                     texture=SPARK_DOT, stretch=0.03)]


def shell_trail(arc, width, fire_rate, fire_size, smoke_size):
    """A burning shell's trail: fire puffs cooling to smoke, a dark smoke column and embers,
    all left hanging along its arc."""
    return [vl.fire_trail('fire', 'tracer', fire_rate, fire_size, 16),
            vl.smoke_trail('smoke', 'tracer', '#3e3a36', '#7a746c', 30, smoke_size, 50, opacity=0.5),
            vl.motes('embers', 'tracer', EMBER, '#a02a08', 40, 0.16, 18, intensity=3.5, core=0.8, speed=1.6,
                     gravity=6.0, texture=SPARK_DOT, stretch=0.04),
            vl.wake('heat', 'tracer', FIRE_LIGHT, '#ff5a14', width=width, ticks=6, opacity=1.0, additive=0.65,
                    texture=vl.FLAME_STRIP, tile=1.6, scroll=4.0)]


# Texture brightness → substance colour: deep violet rim, violet, lilac, a white-hot heart.
SUBSTANCE_RAMP = vl.colours((0.0, '#1c0c40'), (0.3, '#5a2ad8'), (0.6, '#9a74ff'), (0.85, '#dccfff'),
                            (1.0, '#ffffff'))


def substance_blast(id_, count, size, speed, *, lifetime=24, height=1.0, flat=False, start=0, spawn=0.6):
    """A burst of substance: ramp-coloured puffs born glowing that cool into violet haze."""
    e = emitter('burst', lifetime, count, speed, variation=0.3 if flat else 0.5, cone=88 if flat else 180,
                drag=3.0, gravity=-0.6, spawn=spawn, start=size, end=size * 2)
    e.update({'sizeOverLife': vl.keys((0, size * 0.6), (0.15, size * 1.4), (1, size * 2.6)),
              'colourOverLife': vl.colours((0, WHITE), (0.6, WHITE), (0.9, '#7a64c0'), (1, '#4a3c80')),
              'additiveOverLife': vl.keys((0, 0.8), (0.6, 0.7), (0.9, 0.3)),
              'opacityOverLife': vl.keys((0, 0), (0.04, 0.9), (0.6, 0.7), (1, 0)),
              'erosionOverLife': vl.keys((0, 0.05), (0.5, 0.2), (1, 0.85)),
              'heatOverLife': vl.keys((0, 1.05), (0.4, 0.8), (1, 0.45)),
              'turbulence': {'strength': size * 0.3, 'scale': 2.0, 'speed': 2.0}})
    if flat:
        e['coneInnerDegrees'] = 78
        e['spawnInner'] = 0.4
    return layer(id_, 'particles', WHITE, WHITE, 3, count, size, height, startTick=start, blend='additive',
                 texture=vl.PUFF, flipbook=vl.VARIANTS, ramp=SUBSTANCE_RAMP, emitter=e)


def arcs(prefix, width, duration, *, height=1.2, spread=0.7, forks=3, colour='#b49cff', accent=VIOLET,
         intensity=1.8, blue=False):
    """Substance lightning between the cue's ends: a violet bolt with forks and a white core
    tracing the same path (the same seed)."""
    if blue:
        colour, accent = '#9fb4ff', BLUE
    return [vl.lightning(prefix + 'bolt', colour, accent, width * 1.5, duration, spread=spread, forks=forks,
                         height=height, intensity=intensity * 1.3, additive=False),
            vl.lightning(prefix + 'core', '#ffffff', '#e6deff', width * 0.35, duration, spread=spread, forks=0,
                         height=height, intensity=1.8)]


def streaks(id_, colour=SUBSTANCE, accent=SUBSTANCE_HOT, count=12, speed=9.0, size=0.08, lifetime=12, gravity=12.0,
            cone=80, direction=(0, 1, 0), height=0.0, start=0, stretch=0.05, core=0.35, intensity=5.0, spawn=0.15,
            drag=0.7, anchor=None, mode='burst', rate=None, duration=None, sustain=False, follow=None):
    """Hot sparks drawn as streaks along their velocity, white at the core."""
    e = emitter(mode, lifetime, rate or count, speed, variation=0.5, direction=direction, cone=cone, gravity=gravity,
                drag=drag, spawn=spawn, start=size, end=size * 0.45, fade_out=0.55)
    e['stretch'] = stretch
    l = layer(id_, 'particles', colour, accent, duration or lifetime + 2, count, size, height, startTick=start,
              blend='additive', intensity=intensity, core=core, texture=SPARK_DOT, emitter=e)
    if anchor:
        l['anchor'] = anchor
    if sustain:
        l['sustain'] = True
    if follow is not None:
        l['follow'] = follow
    return l


def shards(id_, colour='#dfe6ff', accent=VIOLET_LIGHT, count=24, speed=11.0, size=0.34, lifetime=22, gravity=16.0,
           cone=180, height=1.0, start=0, intensity=2.6, spin=12.0):
    """Spinning slivers of glass thrown out by a burst."""
    return layer(id_, 'particles', colour, accent, lifetime + 2, count, size, height, startTick=start,
                 blend='additive', intensity=intensity, core=0.7, texture=SHARD_ART,
                 emitter=emitter('burst', lifetime, count, speed, variation=0.55, cone=cone, gravity=gravity, drag=0.5,
                                 spawn=0.3, start=size, end=size * 0.6, spin=spin, fade_out=0.45))


def glint(id_, colour, size, duration=5, height=0.0, start=0, anchor=None, intensity=2.6, core=0.9, art_=GLINT,
          **extra):
    """A camera-facing star (or `FLASH_ART` muzzle flash) that burns white at its centre."""
    l = layer(id_, 'billboard', colour, WHITE, duration, 1, size, height, startTick=start, blend='additive',
              intensity=intensity, core=core, texture=art_)
    if anchor:
        l['anchor'] = anchor
    l.update(extra)
    return l


def muzzle(id_, colour, size, duration=3, height=0.0, anchor='source', intensity=2.4, **extra):
    return glint(id_, colour, size, duration, height, anchor=anchor, intensity=intensity, art_=FLASH_ART, **extra)


def orb(id_, colour, accent, size, duration, height=1.0, start=0, intensity=1.0, core=0.8, **extra):
    """A soft camera-facing orb of light in the air (a glow that is not a floating disc)."""
    l = layer(id_, 'glow', colour, accent, duration, 1, size, height, startTick=start, blend='additive',
              orientation='camera', intensity=intensity, core=core)
    l.update(extra)
    return l


def shock(id_, colour, size, duration, height=0.1, start=0, intensity=1.6, accent=WHITE):
    """A thin shock ring racing out over the ground."""
    return layer(id_, 'billboard', colour, accent, duration, 1, size, height, startTick=start, orientation='ground',
                 blend='additive', intensity=intensity, core=0.6, texture=RING_ART,
                 motion={'scale': {'periodTicks': duration * 2, 'min': 0.18, 'max': 1.0, 'easing': 'sine'}})


def scorch(id_, size, duration=240, start=2, height=0.05):
    """A soot mark left on the ground."""
    return layer(id_, 'billboard', WHITE, WHITE, duration, 1, size, height, startTick=start, orientation='ground',
                 blend='normal', intensity=0.85, texture=SCORCH)


def smoke(id_, colour='#5d5a58', accent='#a09c98', count=8, speed=2.2, size=1.8, height=0.4, lifetime=50, start=2,
          spawn=1.2, cone=80, rise=-0.8):
    return puffs(id_, colour, accent, count=count, speed=speed, size=size, height=height, lifetime=lifetime,
                 rise=rise, cone=cone, start=start, spawn=spawn)


# ----- spell heft (the beetles' own painted art, asset.effects.beetles.*) --------------------

GEAR = art('gear-sigil')
ARC_GLYPH = art('arc-sigil')
STEAM_BURST = art('steam-burst')
BOLT = art('lightning')
GOLD = '#ffd890'
GOLD_LIGHT = '#fff3d6'
STEEL = '#cfe0ff'


def sigil(id_, texture, colour, size, duration, height=0.08, spin=320, sustain=False, start=0, intensity=1.4,
          grow=True, follow=None, **extra):
    """A painted glyph laid on the ground, turning slowly and blooming out from the centre. The
    art fades towards its edges, so it is drawn larger than the area it marks."""
    size *= 1.5
    intensity *= 1.8
    motion = {'rotation': {'periodTicks': spin, 'direction': 'clockwise'}}
    if grow and not sustain:
        motion['scale'] = {'periodTicks': max(2, min(duration, 24) * 2), 'min': 0.55, 'max': 1.0,
                           'easing': 'smoothstep'}
    if sustain:
        motion['opacity'] = pulse(80, 0.7, 1.0)
    l = layer(id_, 'billboard', colour, WHITE, duration, 1, size, height, startTick=start, orientation='ground',
              blend='additive', intensity=intensity, core=0.3, texture=texture, motion=motion)
    if sustain:
        l['sustain'] = True
    if follow is not None:
        l['follow'] = follow
    l.update(extra)
    return l


def sprite(id_, texture, colour, size, duration, height=1.0, start=0, intensity=1.6, grow=(0.6, 1.0), **extra):
    """A camera-facing painted sprite that blooms in and fades out."""
    l = layer(id_, 'billboard', colour, WHITE, duration, 1, size * 1.5, height, startTick=start, blend='additive',
              intensity=intensity * 1.6, core=0.4, texture=texture,
              motion={'scale': {'periodTicks': duration * 2, 'min': grow[0], 'max': grow[1], 'easing': 'smoothstep'},
                      'opacity': {'periodTicks': duration * 2, 'min': 0.0, 'max': 1.0, 'easing': 'sine',
                                  'phase': 0.5}})
    l.update(extra)
    return l


def bolt(id_, colour, size, duration=6, height=None, start=0, intensity=2.2):
    """A lightning bolt standing over the target, its foot on the ground."""
    return layer(id_, 'billboard', colour, WHITE, duration, 1, size, size * 0.5 if height is None else height,
                 startTick=start, blend='additive', intensity=intensity * 1.6, core=0.6, texture=BOLT,
                 motion={'opacity': pulse(4, 0.55, 1.0)})


def column(id_, colour, accent, size, height, duration, start=0, intensity=1.2, sustain=False, follow=None):
    """A pillar of light: the mark of a hero spell."""
    l = layer(id_, 'pillar', colour, accent, duration, 1, size, height, startTick=start, blend='additive',
              intensity=intensity * 1.5)
    if sustain:
        l['sustain'] = True
    if follow is not None:
        l['follow'] = follow
    return l


def steam_burst(id_, size, duration=22, height=0.6, start=0, colour=STEAM, intensity=0.7):
    """White steam blasting out of the vents, painted, blooming outward."""
    return sprite(id_, STEAM_BURST, colour, size, duration, height=height, start=start, intensity=intensity,
                  grow=(0.4, 1.1))


def build_casts():
    """Every cast shows: a wind-up at the caster while the spell is prepared (`accepted`).
    Units gather it in their hands over a small glyph; heroes stand in a turning sigil the size
    of a squad, with a light. Keys are used by gen.py."""
    C = {}

    def windup(key, rel, name, texture, colour, accent, sparks_, size, duration, hero=False):
        layers = [
            sigil('glyph', texture, colour, size * 1.8, duration, intensity=1.1 if hero else 0.9),
            orb('hands', colour, accent, size * 0.55, duration, height=2.0 if hero else 1.4, intensity=1.3,
                motion={'scale': pulse(duration * 2, 0.4, 1.1, 'smoothstep')}),
            streaks('rise', sparks_[0], sparks_[1], count=14 if hero else 8, speed=3.2, size=0.08, lifetime=18,
                    gravity=-6.0, cone=24, spawn=size * 0.55, mode='continuous', duration=duration, stretch=0.1,
                    intensity=4.0),
        ]
        if hero:
            layers += [
                ring('rim', colour, accent, size * 2.2, duration, intensity=0.8,
                     motion={'opacity': pulse(12, 0.5, 1.0)}),
                flash('light', colour, accent, size * 4.0, 3.5, duration=duration, height=2.0),
            ]
        C[key] = add('casts/' + rel, effect('effect.beetles.cast.' + rel, name,
                                           name + ': the caster gathers the spell.', duration, layers))

    windup('cast_arc', 'arc', 'Coil wind-up', ARC_GLYPH, VIOLET, VIOLET_LIGHT, (VIOLET_LIGHT, WHITE), 1.8, 24)
    windup('cast_forge', 'forge', 'Forge wind-up', GEAR, SPARK, SPARK_LIGHT, (EMBER, FIRE_LIGHT), 1.8, 24)
    windup('cast_field', 'field', 'Field wind-up', GEAR, BLUE, BLUE_LIGHT, (BLUE_LIGHT, WHITE), 1.8, 24)
    windup('cast_hero_arc', 'hero-arc', 'Coilkeeper wind-up', ARC_GLYPH, VIOLET, VIOLET_LIGHT, (VIOLET_LIGHT, WHITE), 3.0,
           30, hero=True)
    windup('cast_hero_forge', 'hero-forge', 'Forgemaster wind-up', GEAR, SPARK, SPARK_LIGHT, (EMBER, FIRE_LIGHT), 3.0, 30,
           hero=True)
    windup('cast_hero_war', 'hero-war', 'Hornbreaker wind-up', GEAR, GOLD, GOLD_LIGHT, (SPARK, SPARK_LIGHT), 3.0, 24,
           hero=True)

    # The Hornbreaker: a horned colossus of war. His spells are earth, brass and furnace heat.
    C['rush_trail'] = add('hornbreaker/horn-rush-trail', effect(
        'effect.hornbreaker.horn-rush-trail', 'Horn rush',
        'The Hornbreaker thunders forward: dust thrown up behind him and sparks off his horn.', 40, [
            puffs('dust', '#c8b090', '#efe2c8', count=6, speed=2.2, size=1.2, height=0.2, lifetime=26, rise=-0.4,
                  cone=70, mode='continuous', duration=40).__or__({'follow': True}),
            streaks('sparks', SPARK, SPARK_LIGHT, count=10, speed=5, size=0.08, lifetime=12, gravity=14,
                    height=1.6, spawn=0.6, mode='continuous', duration=40, follow=True, intensity=4.0),
            ring('wake', GOLD, GOLD_LIGHT, 3.2, 40, follow=True, intensity=0.6),
        ]))
    C['rush_hit'] = add('hornbreaker/horn-rush-impact', effect(
        'effect.hornbreaker.horn-rush-impact', 'Horn rush impact',
        'The horn lands: a white-gold star, a wide shock ring, a crack in the ground, rocks and dust.', 60, [
            glint('star', GOLD_LIGHT, 5.0, duration=7, height=1.4, intensity=3.2),
            sigil('crack', FRACTURE_TEX, GOLD, 4.0, 60, spin=4000, intensity=1.2),
            shock('shock', GOLD_LIGHT, 11.0, 16, intensity=2.2),
            shock('shock2', SPARK, 7.0, 20, start=4, height=0.12),
            streaks('sparks', SPARK, WHITE, count=26, speed=12, size=0.09, lifetime=16, gravity=16, cone=80,
                    height=1.0, intensity=4.5),
            puffs('debris', '#5a4c40', '#8a7a68', count=10, speed=8, size=0.5, height=0.4, lifetime=26, rise=18,
                  cone=55, spawn=0.8),
            puffs('dust', '#c8b090', '#efe2c8', count=10, speed=3.4, size=1.6, height=0.2, lifetime=36, rise=-0.4,
                  cone=85),
            flash('light', GOLD, GOLD_LIGHT, 10.0, 8.0, duration=10, height=1.4),
        ]))
    C['overwhelm'] = add('hornbreaker/overwhelm-hit', effect(
        'effect.hornbreaker.overwhelm-hit', 'Overwhelming blow',
        'An enhanced blow: a gold slash of light, a shock ring and sparks.', 26, [
            glint('star', GOLD_LIGHT, 3.2, duration=6, height=1.2, intensity=3.0),
            shock('shock', GOLD, 6.0, 14, intensity=1.8),
            streaks('sparks', SPARK, WHITE, count=14, speed=9, size=0.08, lifetime=12, gravity=14, cone=90,
                    height=1.0, intensity=4.0),
        ]))
    C['steadfast'] = add('hornbreaker/steadfast', effect(
        'effect.hornbreaker.steadfast', 'Steadfast',
        'Allies standing with the Hornbreaker: a slow gold gear turning at their feet and a warm glow.', 80, [
            sigil('gear', GEAR, GOLD, 2.0, 80, sustain=True, spin=480, intensity=1.0, follow=True),
            ring('ring', GOLD, GOLD_LIGHT, 2.0, 80, sustain=True, follow=True, intensity=0.7),
            glow('pool', GOLD, GOLD_LIGHT, 1.8, 80, height=0.08, sustain=True, follow=True, intensity=0.5,
                 motion={'opacity': pulse(60, 0.5, 1.0)}),
        ], loop=True))
    C['colossus'] = add('hornbreaker/colossus', effect(
        'effect.hornbreaker.colossus', 'Colossus',
        'The Hornbreaker opens every furnace vent and grows: a pillar of fire and light, a great brass sigil '
        'slamming down, steam blasting out, two shock rings and a flare that lights the battlefield.', 60, [
            column('pillar', FIRE, GOLD_LIGHT, 3.0, 18, 36, intensity=1.2),
            column('core', GOLD_LIGHT, WHITE, 1.0, 20, 28, intensity=1.4),
            sigil('sigil', GEAR, GOLD, 9.0, 60, spin=180, intensity=1.6),
            steam_burst('steam', 9.0, duration=30, height=1.2),
            glint('star', GOLD_LIGHT, 8.0, duration=9, height=3.0, intensity=3.2),
            shock('shock', GOLD_LIGHT, 18.0, 20, intensity=2.4),
            shock('shock2', FIRE_LIGHT, 12.0, 26, start=6, height=0.12),
            streaks('embers', EMBER, FIRE_LIGHT, count=46, speed=13, size=0.1, lifetime=24, gravity=10, cone=100,
                    height=1.0, intensity=4.5),
            flash('light', FIRE, GOLD_LIGHT, 18.0, 12.0, duration=16, height=3.0),
        ]))
    C['colossus_form'] = add('hornbreaker/colossus-form', effect(
        'effect.hornbreaker.colossus-form', 'Colossus form',
        'While the Hornbreaker is a colossus: furnace heat glowing at his feet, embers and steam pouring off him, a '
        'brass gear turning beneath him.', 80, [
            sigil('gear', GEAR, FIRE, 4.2, 80, sustain=True, spin=200, intensity=0.8, follow=True),
            ring('ring', FIRE, FIRE_LIGHT, 4.4, 80, sustain=True, follow=True, intensity=0.8,
                 motion={'opacity': pulse(20, 0.5, 1.0)}),
            streaks('embers', EMBER, FIRE_LIGHT, count=10, speed=3.0, size=0.09, lifetime=22, gravity=-6, cone=30,
                    spawn=1.6, mode='continuous', duration=80, sustain=True, follow=True, stretch=0.1,
                    intensity=4.0, height=1.0),
            puffs('steam', count=3, speed=1.6, size=1.0, height=2.4, lifetime=30, rise=-2.0, mode='continuous',
                  duration=80).__or__({'sustain': True, 'follow': True}),
            flash('light', FIRE, FIRE_LIGHT, 8.0, 2.4, duration=80, height=2.0, sustain=True, follow=True,
                  motion={'opacity': pulse(16, 0.6, 1.0)}),
        ], loop=True))
    return C


def build():
    """The troops' weapons (keys used by gen.py), and the machines' and Hornwatch's."""
    W = {}
    W['bolt'] = add('weapons/bolt', effect(
        'effect.beetles.weapons.bolt', 'Pressure bolt', 'A brass bolt with a glowing substance head.', 40,
        [missile_model('asset.models.effects.beetles-bolt', 0.9)]))
    W['muzzle'] = add('weapons/bolter-muzzle', effect(
        'effect.beetles.weapons.bolter-muzzle', 'Repeater volley',
        'A ragged violet-white muzzle flash, a spit of sparks, steam from the canister and a hot violet tracer on '
        'every shot.', 40, [
            muzzle('flash', '#a888ff', 2.0, duration=3, intensity=3.0),
            glint('glint', SUBSTANCE, 2.8, duration=4, anchor='source', intensity=2.4),
            streaks('sparks', count=9, speed=10.0, size=0.08, lifetime=9, gravity=10, cone=55, anchor='source'),
            puffs('steam', count=3, speed=1.4, size=0.8, height=0.1, lifetime=26, rise=-2.0).__or__(
                {'anchor': 'source'}),
            head('#8a5cff', WHITE),
            *substance_trail('#7a4cff', '#c8b4ff'),
        ]))
    W['bolt_hit'] = add('weapons/bolt-impact', effect(
        'effect.beetles.weapons.bolt-impact', 'Bolt impact',
        'A violet star and a spray of hot sparks where a bolt strikes.', 24, [
            glint('glint', SUBSTANCE, 2.2, duration=5, height=0.3, intensity=2.8),
            streaks('sparks', count=12, speed=8.5, size=0.08, lifetime=11, gravity=14, cone=80, height=0.3),
            puffs('dust', '#b8a890', '#e0d6c4', count=2, speed=1.2, size=0.55, height=0.0, lifetime=18, rise=-0.6),
            vl.ring('ring', VIOLET, 3.0, duration=8, intensity=0.9),
        ]))
    W['drill'] = add('weapons/drill-strike', effect(
        'effect.beetles.weapons.drill-strike', 'Drill strike',
        'A white-hot glint, a fan of orange drill sparks and a burst of grit where the siege drill bites.', 30, [
            glint('glint', EMBER, 1.6, duration=4, height=0.5),
            streaks('sparks', EMBER, FIRE_LIGHT, count=24, speed=11, size=0.07, lifetime=14, gravity=22, cone=95,
                    height=0.4),
            puffs('grit', '#a49076', '#d8c8ac', count=6, speed=2.6, size=0.8, height=0.0, lifetime=24, rise=-0.4,
                  cone=85),
        ]))
    W['bash'] = add('weapons/shield-bash', effect(
        'effect.beetles.weapons.shield-bash', 'Piston hammer blow',
        'A clang of sparks, a glint and a puff of steam from the hammer.', 26, [
            glint('glint', SPARK, 1.4, duration=4, height=0.6),
            streaks('sparks', SPARK, SPARK_LIGHT, count=12, speed=8, size=0.07, lifetime=12, gravity=18, cone=90,
                    height=0.5),
            puffs('steam', count=3, speed=1.0, size=0.7, height=0.3, lifetime=22, rise=-1.6),
        ]))
    W['arc'] = add('weapons/arc', effect(
        'effect.beetles.weapons.arc', 'Substance arc',
        'A crackling violet arc from the coil to its target: a soft halo, a violet arc and a white core, with a star '
        'at the coil.', 20, [
            *arcs('arc-', 0.8, 8, height=0.0, spread=0.6, forks=2),
            glint('coil', SUBSTANCE, 1.6, duration=5, anchor='source', intensity=2.0),
            streaks('sparks', count=6, speed=6, size=0.06, lifetime=8, gravity=4, cone=120, anchor='source'),
        ]))
    W['arc_hit'] = add('weapons/arc-impact', effect(
        'effect.beetles.weapons.arc-impact', 'Arc impact', 'Violet sparks crawl over whatever the arc strikes.', 24, [
            glint('glint', SUBSTANCE, 1.5, duration=5, height=0.6),
            streaks('sparks', count=14, speed=7, size=0.06, lifetime=12, gravity=4, cone=140, height=0.6),
        ]))
    W['syringe'] = add('weapons/syringe', effect(
        'effect.beetles.weapons.syringe', 'Syringe dart', 'A hissing dart of pressurised tonic.', 40, [
            puffs('hiss', count=2, speed=0.8, size=0.45, height=0.1, lifetime=18, rise=-1.4).__or__({'anchor': 'source'}),
            glint('flash', BLUE, 0.9, duration=3, anchor='source', intensity=1.6),
            head('#8fa6ff', WHITE, size=0.13, length=7, intensity=4.0),
            vl.wake('wake', 'tracer', BLUE_LIGHT, BLUE, width=0.4, ticks=7, opacity=1.0, additive=0.7),
            vl.motes('mist', 'tracer', '#b8c8ff', BLUE, 30, 0.28, 14, intensity=1.3, speed=0.5),
            vl.drips('drops', 'tracer', '#8fa6ff', '#5b7bff', 10, 0.14, additive=0.5),
        ]))
    W['syringe_hit'] = add('weapons/syringe-impact', effect(
        'effect.beetles.weapons.syringe-impact', 'Dart splash', 'A small blue splash.', 18, [
            glint('glint', BLUE, 0.9, duration=4, height=0.3),
            streaks('drops', '#8fa6ff', '#dde6ff', count=7, speed=4, size=0.06, lifetime=12, gravity=14, height=0.3),
        ]))
    W['zap'] = add('weapons/prong-zap', effect(
        'effect.beetles.weapons.prong-zap', 'Prong discharge',
        'A short blue discharge between the projector prongs and the target.', 16, [
            *arcs('arc-', 0.6, 6, height=0.0, spread=0.4, forks=1, blue=True),
            glint('flash', BLUE, 1.3, duration=4, anchor='source', intensity=1.8),
        ]))
    build_machines()
    return W


def build_machines():
    """The Steamwing's, Bombard's and Juggernaut's guns, the Hornwatch's shot and the Brood
    Den's pressure (ids kept; gen.py binds them)."""
    # Steamwing: a violet bolt from the chin emitter.
    add_at('units/steamwing-bolt', effect(
        'effect.beetles.steamwing-bolt', 'Steamwing substance bolt',
        "The violet bolt the Steamwing's chin emitter fires (geometry 2 of the Steamwing asset). Straight, fast, "
        'no arc.', 40, [missile_model('asset.models.units.beetles-steamwing', 1.0, index=2)]))
    add_at('units/steamwing-tracer', effect(
        'effect.beetles.steamwing-tracer', 'Steamwing beam',
        'A searing violet beam from the emitter to the target for an instant, and a hot tracer riding the bolt.', 40, [
            beam('halo', VIOLET, VIOLET_LIGHT, 0.9, 4, height=0.0, spread=0.12, intensity=0.4),
            beam('beam', '#b89cff', WHITE, 0.22, 4, height=0.0, spread=0.08),
            head('#c4a8ff', WHITE, size=0.26, length=8, intensity=5.0),
            *substance_trail(VIOLET, '#d8c8ff', width=0.7),
            vl.smoke_trail('steam', 'tracer', '#e8ecf2', '#ffffff', 30, 0.8, 24, opacity=0.45, rise=-1.0),
        ]))
    add_at('units/steamwing-muzzle', effect(
        'effect.beetles.steamwing-muzzle', 'Steamwing emitter flash',
        'The chin emitter flares: a violet-white star and a spit of sparks.', 24, [
            muzzle('flash', '#c4a8ff', 1.4, duration=3, anchor=None),
            glint('glint', SUBSTANCE, 2.0, duration=4, intensity=1.8),
            streaks('sparks', count=8, speed=7, size=0.06, lifetime=9, gravity=4, cone=120),
        ]))
    add_at('units/steamwing-impact', effect(
        'effect.beetles.steamwing-impact', 'Steamwing bolt impact',
        'A violet star, a thin shock ring and crackling sparks where the bolt lands.', 26, [
            glint('glint', SUBSTANCE, 1.9, duration=5, height=0.4),
            streaks('sparks', count=14, speed=8, size=0.07, lifetime=12, gravity=8, cone=140, height=0.4),
            shock('ring', VIOLET, 2.6, 10, intensity=1.2),
        ]))
    # Bombard: a mortar of fire-and-substance shells on a high arc.
    add_at('units/bombard-shell', effect(
        'effect.units.bombard-shell', 'Bombard shell', 'The mortar shell (geometry 2 of the Bombard asset).', 40,
        [missile_model('asset.models.units.beetles-bombard', 1.0, arc=9, index=2)]))
    add_at('units/bombard-trail', effect(
        'effect.units.bombard-trail', 'Bombard shell trail',
        'A burning fuse riding the shell up its arc, trailing embers and a violet glow.', 40, [
            head(EMBER, FIRE_LIGHT, size=0.45, length=1.6, arc=9, intensity=4.0),
            *shell_trail(9, 1.2, 70, 1.3, 1.5),
        ]))
    add_at('units/bombard-launch', effect(
        'effect.units.bombard-launch', 'Bombard launch',
        'The mortar coughs: a ragged flash, a light, sparks and a column of smoke from the barrel.', 96, [
            flash('light', '#ffa140', FIRE_LIGHT, 12.0, 16.0, duration=7, height=0.0),
            muzzle('flash', '#ffc865', 2.4, duration=4, anchor=None),
            glint('glint', EMBER, 2.6, duration=5),
            orb('charge', VIOLET, VIOLET_LIGHT, 1.4, 8, height=0.0, intensity=0.8),
            streaks('sparks', EMBER, FIRE_LIGHT, count=18, speed=11, size=0.08, lifetime=14, gravity=14, cone=40),
            smoke('smoke-column', '#7e786f', '#b0aaa2', count=12, speed=5.5, size=1.0, height=0.0, lifetime=80,
                  start=1, spawn=0.3, cone=18, rise=-0.6),
        ]))
    add_at('units/bombard-impact', effect(
        'effect.units.bombard-impact', 'Bombard impact',
        'The shell bursts: a fireball with a violet heart, a shock ring, hot fragments, flying dirt, smoke and a '
        'scorch on the ground.', 240, [
            flash('light', '#ff9a3c', FIRE_LIGHT, 16.0, 20.0, duration=10, height=1.5),
            explosion('fireball', 8.5, height=2.2),
            glint('glint', '#ffd08a', 5.0, duration=6, height=1.2),
            orb('heart', VIOLET, VIOLET_LIGHT, 2.6, 12, height=1.0, intensity=0.9),
            shock('shock', FIRE_LIGHT, 11.0, 14),
            streaks('sparks', EMBER, FIRE_LIGHT, count=34, speed=14, size=0.09, lifetime=20, gravity=20, cone=85,
                    height=0.3),
            streaks('substance', count=14, speed=9, size=0.07, lifetime=18, gravity=6, cone=110, height=0.3),
            puffs('dirt', '#6b5641', '#8a7458', count=16, speed=9, size=0.9, height=0.0, lifetime=40, rise=18,
                  cone=50, spawn=0.6),
            smoke('smoke', '#6f6a62', '#a8a29a', count=10, speed=2.2, size=2.0, height=0.4, lifetime=80, start=5,
                  spawn=1.8),
            scorch('scorch', 5.0),
        ]))
    # Juggernaut: two cannons firing flat shells.
    add_at('units/juggernaut-shell', effect(
        'effect.units.juggernaut-shell', 'Juggernaut shell', 'A cannon shell (geometry 2 of the Juggernaut asset).',
        40, [missile_model('asset.models.units.beetles-juggernaut', 1.6, arc=4.5, index=2)]))
    add_at('units/juggernaut-trail', effect(
        'effect.units.juggernaut-trail', 'Juggernaut shell trail', 'A glowing shell trailing fire across the field.', 40, [
            head(EMBER, FIRE_LIGHT, size=0.55, length=2.2, arc=4.5, intensity=4.5),
            *shell_trail(4.5, 1.4, 80, 1.5, 1.7),
        ]))
    add_at('units/juggernaut-muzzle', effect(
        'effect.units.juggernaut-muzzle', 'Juggernaut cannon',
        'A cannon roars: a big ragged flash, a star, a smoke ring facing the eye, sparks and rolling smoke.', 72, [
            flash('light', '#ffa845', FIRE_LIGHT, 13.0, 16.0, duration=7, height=0.0),
            muzzle('flash', '#ffcf70', 3.2, duration=4, anchor=None),
            glint('glint', EMBER, 3.4, duration=5, intensity=2.0),
            layer('smoke-ring', 'ring', '#9a948a', '#c8c2b8', 26, 1, 3.8, 0, startTick=1, blend='normal',
                  orientation='camera', intensity=0.8),
            streaks('sparks', EMBER, FIRE_LIGHT, count=20, speed=12, size=0.08, lifetime=14, gravity=12, cone=45),
            smoke('smoke', '#8a847a', '#bdb7ae', count=10, speed=2.6, size=1.2, height=0.0, lifetime=66, start=1,
                  spawn=0.4),
        ]))
    add_at('units/juggernaut-impact', effect(
        'effect.units.juggernaut-impact', 'Juggernaut shell impact',
        'A heavy shell bursts: a tall fireball, a second blast, a shock ring, fragments, debris, smoke and a scorch.',
        240, [
            flash('light', '#ffa040', FIRE_LIGHT, 18.0, 20.0, duration=10, height=1.5),
            explosion('fireball', 10.0, height=3.2),
            explosion('fireball-side', 7.0, height=2.4, start=3, offset={'x': 1.2, 'y': 0, 'z': -0.8}),
            glint('glint', '#ffd08a', 6.0, duration=6, height=1.4),
            shock('shock', FIRE_LIGHT, 12.0, 14),
            streaks('sparks', EMBER, FIRE_LIGHT, count=40, speed=15, size=0.1, lifetime=22, gravity=20, cone=80,
                    height=0.6),
            puffs('debris', '#3a3430', '#5a524c', count=18, speed=10, size=0.5, height=0.6, lifetime=36, rise=20,
                  cone=60, spawn=0.6),
            smoke('dust', '#8a7c6a', '#b4a894', count=12, speed=3.5, size=2.2, height=0.5, lifetime=90, start=4,
                  spawn=2.0),
            scorch('scorch', 6.0),
        ]))
    add_at('units/juggernaut-crash', effect(
        'effect.units.juggernaut-crash', 'Juggernaut crash',
        'The walking fortress hits the ground: the furnace bursts in a violet flash, sparks and debris fly, and a '
        'ring of dust rolls out.', 120, [
            flash('light', VIOLET, VIOLET_LIGHT, 16.0, 18.0, duration=14, height=2.0),
            glint('flare', SUBSTANCE, 7.0, duration=8, height=2.5),
            orb('furnace', VIOLET, VIOLET_LIGHT, 3.5, 20, height=2.0, intensity=0.9),
            shock('shock', '#c9b8ff', 14.0, 16),
            layer('dust-ring', 'ring', '#8b7d6b', '#b4a894', 30, 1, 13, 0.1, blend='normal'),
            streaks('sparks', SPARK, SPARK_LIGHT, count=36, speed=12, size=0.09, lifetime=24, gravity=18, cone=85,
                    height=1.0),
            streaks('substance', count=22, speed=10, size=0.08, lifetime=22, gravity=4, cone=120, height=2.0),
            puffs('debris', '#3a3430', '#5a524c', count=16, speed=9, size=0.55, height=1.0, lifetime=36, rise=20,
                  cone=70, spawn=1.0),
            smoke('dust', '#8a7c6a', '#b4a894', count=16, speed=5, size=2.4, height=0.3, lifetime=86, start=0,
                  spawn=3.0, cone=85),
            scorch('scorch', 9.0, start=6),
        ]))
    # Hornwatch: the turret's repeater fires substance bolts (the crew's tracer flies from
    # the gunner); the turret flashes on every shot.
    add_at('buildings/hornwatch-muzzle', effect(
        'effect.buildings.hornwatch-muzzle', 'Hornwatch shot',
        "The Hornwatch's turret repeater firing, on the clip's `fire` event: a ragged violet-white flash at the "
        'mouth, a star, a spit of sparks and a breath of steam.', 48, [
            muzzle('flash', '#c4a8ff', 2.2, duration=4, anchor=None),
            glint('glint', SUBSTANCE, 2.8, duration=5, intensity=2.0),
            streaks('sparks', count=12, speed=9, size=0.08, lifetime=12, gravity=10, cone=60),
            puffs('steam', count=4, speed=1.6, size=0.9, height=0.0, lifetime=40, rise=-1.6),
        ]))
    # Arc Tower: the crown jumps and the sphere discharges on every arc its coil throws.
    add_at('buildings/arc-tower-discharge', effect(
        'effect.buildings.arc-tower-discharge', 'Arc Tower discharge',
        "The Arc Tower's sphere discharges on the clip's `fire` event: a violet-white star, a crown of crackling "
        'streaks and a thin ring of field energy.', 30, [
            flash('light', VIOLET, VIOLET_LIGHT, 9.0, 9.0, duration=6, height=0.0),
            glint('star', '#c4a8ff', 3.4, duration=6, intensity=2.6),
            orb('sphere', VIOLET, VIOLET_LIGHT, 1.6, 8, height=0.0, intensity=0.9),
            streaks('crackle', count=18, speed=8, size=0.07, lifetime=10, gravity=0, cone=180, drag=2.0),
            layer('ring', 'ring', VIOLET_LIGHT, WHITE, 10, 1, 2.6, 0, blend='additive', orientation='camera',
                  intensity=1.2),
        ]))
    add_at('buildings/arc-tower-coil', effect(
        'effect.buildings.arc-tower-coil', 'Arc Tower coil charge',
        'While the tower fights, substance surges through its glass coil: a violet glow and sparks climbing it.',
        40, [
            orb('glow', VIOLET, VIOLET_LIGHT, 1.4, 40, height=0.0, sustain=True, intensity=0.6,
                motion={'opacity': pulse(8, 0.5, 1.0)}),
            streaks('climb', count=6, speed=2.5, size=0.05, lifetime=12, gravity=-6, cone=20, spawn=0.4,
                    mode='continuous', duration=40, sustain=True, stretch=0.06, intensity=3.5),
        ], loop=True))
    # Brood Den: while it pressurises the network, its gauge glows and pulses.
    add_at('buildings/brood-den-pressure', effect(
        'effect.buildings.brood-den-pressure', 'Brood Den pressure',
        "The den's back-lit gauge glows blue-violet, swelling and ebbing with the pressure while it powers the "
        'network (D65, D66).', 80, [
            flash('glow', VIOLET, '#b89cff', 5.0, 4.0, duration=80, height=0.0, sustain=True,
                  motion={'opacity': pulse(40, 0.45, 1.0)}),
            orb('dial', VIOLET, VIOLET_LIGHT, 0.9, 80, height=0.0, sustain=True, intensity=0.7,
                motion={'opacity': pulse(40, 0.4, 1.0)}),
        ], loop=True))
    add_at('buildings/brood-den-vent', effect(
        'effect.buildings.brood-den-vent', 'Brood Den pressure vent',
        'The needle reaches the top of its dial: a sharp white jet of steam from the stack and a violet flicker at '
        'its mouth.', 64, [
            puffs('puff', '#e2dfe6', '#f8f7fa', count=14, speed=3.6, size=0.8, height=0.0, lifetime=56, rise=-0.4,
                  cone=22, spawn=0.15),
            glint('flicker', SUBSTANCE, 1.2, duration=6, intensity=1.4),
            streaks('motes', count=6, speed=3.0, size=0.05, lifetime=18, gravity=-2, cone=30),
        ]))


def build_abilities():
    A = {}
    # Shieldbearer
    A['brace'] = add('shieldbearer/lock-shields', effect(
        'effect.beetles.lock-shields', 'Locked shields',
        'A blue-violet ring and a steady glow around a shieldbearer bracing behind a locked tower shield.', 80, [
            ring('ring', BLUE, BLUE_LIGHT, 2.8, 80, sustain=True, intensity=0.8, motion={'opacity': pulse(40, 0.6, 1.0)}),
            glow('glow', BLUE, BLUE_LIGHT, 2.4, 80, height=0.1, sustain=True, motion={'scale': pulse(40, 0.9, 1.08)}),
            ground('plate', WARD_TEX, BLUE, BLUE_LIGHT, 3.6, 80, sustain=True, intensity=1.4,
                   motion={'opacity': pulse(60, 0.5, 0.9)}),
        ], loop=True))
    A['brace_lock'] = add('shieldbearer/lock-shields-clang', effect(
        'effect.beetles.lock-shields-clang', 'Shields lock', 'Sparks and a vent of steam as the shield clamps lock.', 30, [
            glint('glint', STEEL, 2.6, duration=6, height=1.0, intensity=3.0),
            streaks('sparks', SPARK, SPARK_LIGHT, count=18, speed=8, size=0.08, lifetime=12, gravity=16, height=0.8,
                    intensity=4.5),
            steam_burst('steam', 3.2, duration=20, height=0.6),
            sigil('gear', GEAR, BLUE, 2.4, 30, spin=90, intensity=1.0),
            shock('ring', BLUE_LIGHT, 5.0, 12, intensity=1.8),
        ]))
    # Breacher
    A['crack'] = add('breacher/armor-crack', effect(
        'effect.beetles.armor-crack', 'Cracked armour',
        'Orange glints and a fracture glyph on armour the drill has opened.', 40, [
            ground('crack', FRACTURE_TEX, FIRE, FIRE_LIGHT, 3.2, 40, sustain=True, intensity=1.6,
                   motion={'opacity': pulse(40, 0.6, 1.0)}),
            streaks('glints', FIRE, FIRE_LIGHT, count=5, speed=1.8, size=0.08, lifetime=14, gravity=6, height=1.2,
                    spawn=0.6, mode='continuous', duration=40, sustain=True, stretch=0.06, intensity=4.0),
        ], loop=True))
    A['crack_hit'] = add('breacher/armor-crack-hit', effect(
        'effect.beetles.armor-crack-hit', 'Armour cracks',
        'The drill bites through: a hot star, a burst of plate chips and orange sparks.', 20, [
            glint('star', FIRE_LIGHT, 2.4, duration=5, height=1.0, intensity=3.0),
            shards('chips', '#ffe2c0', FIRE_LIGHT, count=10, speed=8, size=0.26, lifetime=16, height=1.0),
            streaks('sparks', EMBER, FIRE_LIGHT, count=12, speed=8, size=0.07, lifetime=12, gravity=16, height=1.0,
                    intensity=4.0),
        ]))
    # Arcwright
    A['surge_hop'] = add('arcwright/arc-surge-hop', effect(
        'effect.beetles.arc-surge-hop', 'Arc surge hop',
        'A thick violet arc that jumps from victim to victim, flashing on each.', 18, [
            beam('halo', VIOLET, VIOLET_LIGHT, 1.5, 8, height=1.2, spread=0.6, intensity=0.35),
            beam('arc', '#9a74ff', '#efe8ff', 0.55, 10, height=1.2, spread=0.6),
            beam('core', '#f8f4ff', WHITE, 0.16, 8, height=1.2, spread=0.6),
            bolt('bolt', '#efe8ff', 5.5, duration=8, intensity=2.6),
            glint('glint', '#d8ccff', 3.2, duration=6, height=1.0, intensity=3.0),
            streaks('sparks', count=20, speed=10, size=0.08, lifetime=12, height=1.0, gravity=6, cone=140,
                    intensity=4.5),
            flash('flash', VIOLET, VIOLET_LIGHT, 6.0, 8.0, duration=6, height=1.0),
        ]))
    A['static_zone'] = add('arcwright/static-field', effect(
        'effect.beetles.static-field', 'Static field',
        'A crackling violet field on the ground: a turning glyph, a bright rim and sparks that never stop.', 200, [
            ring('rim', VIOLET, VIOLET_LIGHT, 8.0, 200, sustain=True, intensity=0.9, motion={'opacity': pulse(20, 0.6, 1.0)}),
            sigil('glyph', ARC_GLYPH, VIOLET, 6.0, 200, sustain=True, spin=160, intensity=1.1),
            bolt('strike', '#e0d6ff', 7.0, duration=8),
            glint('star', '#e0d6ff', 4.0, duration=7, height=1.0, intensity=3.0),
            streaks('crackle', '#9a78ff', SUBSTANCE_HOT, count=14, speed=3.5, size=0.06, lifetime=10, gravity=4,
                    cone=80, spawn=3.6, mode='continuous', duration=200, sustain=True, stretch=0.05, height=0.2),
            flash('light', VIOLET, VIOLET_LIGHT, 7.0, 3.0, duration=200, height=1.0, sustain=True,
                  motion={'opacity': pulse(12, 0.5, 1.0)}),
        ]))
    A['static_hit'] = add('arcwright/static-shock', effect(
        'effect.beetles.static-shock', 'Static shock', 'A small violet spark on a unit the field bites.', 14, [
            bolt('bolt', VIOLET_LIGHT, 2.4, duration=4, intensity=1.6),
            glint('glint', '#d8ccff', 1.6, duration=4, height=0.8),
            streaks('sparks', count=8, speed=6, size=0.06, lifetime=8, height=0.8, gravity=4, cone=140, intensity=4.0),
        ]))
    # Mender
    A['mend'] = add('mender/mend', effect(
        'effect.beetles.mend', 'Mend',
        'Blue-violet tonic glows up through the patient while white steam vents from the patched seams.', 40, [
            column('column', BLUE, '#e4ebff', 1.6, 7.0, 20, intensity=1.0),
            sigil('gear', GEAR, BLUE, 2.2, 32, spin=200, intensity=1.0),
            orb('glow', BLUE, BLUE_LIGHT, 2.2, 30, height=1.0, intensity=1.0),
            layer('motes', 'particles', '#b8c6ff', WHITE, 36, 18, 0.24, 0.3, blend='additive', texture=SPARK_DOT,
                  intensity=4.0, core=0.8, emitter=emitter('burst', 30, 18, 3.0, gravity=-3.0, spawn=0.8, start=0.24,
                                                         end=0.06, cone=30)),
            puffs('steam', count=4, speed=1.4, size=0.8, height=0.6, lifetime=28, rise=-1.8),
            flash('flash', BLUE, BLUE_LIGHT, 5.0, 6.0, duration=10, height=1.0),
        ]))
    A['weld'] = add('mender/field-repair', effect(
        'effect.beetles.field-repair', 'Welding torch',
        'A white-hot weld point spraying orange sparks over the plating being repaired.', 40, [
            glint('point', '#fff4d8', 1.2, duration=40, height=1.2, core=1.0,
                  motion={'opacity': pulse(4, 0.5, 1.0)}),
            streaks('spray', SPARK, SPARK_LIGHT, count=20, speed=7.5, size=0.06, lifetime=14, gravity=18, cone=70,
                    height=1.2, mode='continuous', duration=40),
            flash('flash', '#fff0d0', WHITE, 4.0, 4.0, duration=40, height=1.2, motion={'opacity': pulse(4, 0.4, 1.0)}),
        ]))
    A['patch'] = add('tinker/patch-up', effect(
        'effect.beetles.patch-up', 'Patched up', 'A clatter of sparks and a puff of steam as a machine is patched.', 28, [
            glint('glint', SPARK_LIGHT, 2.2, duration=6, height=1.0, intensity=3.0),
            sigil('gear', GEAR, SPARK, 2.0, 28, spin=120, intensity=1.0),
            streaks('sparks', SPARK, SPARK_LIGHT, count=18, speed=7, size=0.08, lifetime=12, gravity=16, height=1.0,
                    intensity=4.5),
            steam_burst('steam', 2.4, duration=18, height=0.8),
        ]))
    # Warden
    A['barrier'] = add('warden/barrier-field', effect(
        'effect.beetles.barrier-field', 'Barrier field',
        'The violet pane of a field projector: a bright rim around the warden and a shimmering dome.', 80, [
            ring('rim', VIOLET, '#efe8ff', 4.4, 80, height=0.3, sustain=True, intensity=0.9,
                 motion={'opacity': pulse(30, 0.6, 1.0)}),
            ring('inner', BLUE, BLUE_LIGHT, 3.6, 80, height=1.4, sustain=True, motion={'opacity': pulse(40, 0.25, 0.55)}),
            orb('dome', VIOLET, VIOLET_LIGHT, 3.4, 80, height=1.6, sustain=True, intensity=0.32, core=0.2,
                motion={'opacity': pulse(40, 0.5, 1.0)}),
            flash('light', VIOLET, VIOLET_LIGHT, 6.0, 2.0, duration=80, height=1.6, sustain=True),
        ], loop=True))
    A['barrier_up'] = add('warden/barrier-raise', effect(
        'effect.beetles.barrier-raise', 'Barrier raised', 'The projector flares as its pane comes up.', 24, [
            shock('wave', VIOLET_LIGHT, 8.0, 14, intensity=2.0),
            sigil('glyph', ARC_GLYPH, VIOLET, 4.0, 24, spin=120, intensity=1.0),
            glint('glint', '#e0d6ff', 4.0, duration=7, height=1.4, intensity=3.0),
            streaks('sparks', count=18, speed=7, size=0.08, lifetime=12, height=1.2, gravity=4, cone=140, intensity=4.5),
            flash('flash', VIOLET, VIOLET_LIGHT, 7.0, 8.0, duration=8, height=1.2),
        ]))
    A['bulwark'] = add('warden/bulwark-pulse', effect(
        'effect.beetles.bulwark-pulse', 'Bulwark pulse', 'A blue shockwave of field energy rolling out from the warden.', 30, [
            shock('wave', BLUE_LIGHT, 16.0, 20, intensity=2.2),
            shock('wave2', VIOLET_LIGHT, 11.0, 16, start=4, height=0.4, intensity=1.8),
            sigil('gear', GEAR, BLUE, 8.0, 30, spin=90, intensity=1.2),
            column('column', BLUE, '#e4ebff', 2.4, 10.0, 20, intensity=1.0),
            steam_burst('steam', 6.0, duration=22, height=0.8),
            glint('glint', '#e4ebff', 4.6, duration=7, height=1.2, intensity=3.0),
            streaks('sparks', '#8fa6ff', BLUE_LIGHT, count=18, speed=9, size=0.07, lifetime=12, height=1.0, gravity=2,
                    cone=100, direction=(0, 0.2, 0)),
            flash('flash', BLUE, BLUE_LIGHT, 9.0, 8.0, duration=10, height=1.2),
        ]))
    A['ward'] = add('warden/bulwark-shield', effect(
        'effect.beetles.bulwark-shield', 'Field shield', 'A thin blue shell of field energy around a protected unit.', 60, [
            orb('shell', BLUE, BLUE_LIGHT, 2.0, 60, height=1.1, sustain=True, intensity=0.5, core=0.25,
                motion={'opacity': pulse(30, 0.5, 1.0)}),
            ground('ward', WARD_TEX, BLUE, BLUE_LIGHT, 2.8, 60, sustain=True, intensity=1.4,
                   motion={'opacity': pulse(30, 0.55, 0.95)}),
        ], loop=True))
    # Juggernaut
    A['broadside'] = add('juggernaut/broadside', effect(
        'effect.beetles.broadside', 'Broadside',
        'A rolling barrage: three fireballs walk across the target area with shock rings, hot fragments, smoke and '
        'scorched ground.', 240, [
            explosion('blast1', 5.5, height=1.2),
            explosion('blast2', 5.0, height=1.2, start=6, offset={'x': 2.2, 'y': 0, 'z': 1.4}),
            explosion('blast3', 5.0, height=1.2, start=12, offset={'x': -2.0, 'y': 0, 'z': -1.6}),
            flash('flash', FIRE, FIRE_LIGHT, 12.0, 9.0, duration=18, height=1.0),
            glint('glint', '#ffd08a', 4.5, duration=6, height=1.0),
            shock('shock', FIRE_LIGHT, 8.0, 16),
            shock('shock2', FIRE_LIGHT, 6.0, 14, start=6, height=0.12),
            streaks('sparks', EMBER, FIRE_LIGHT, count=40, speed=13, size=0.09, lifetime=22, gravity=18, cone=85),
            smoke('smoke', count=10, speed=2.2, size=2.2, height=0.4, lifetime=50, start=4, spawn=2.5, cone=90),
            scorch('scorch', 6.0, start=4),
            steam_burst('steam', 8.0, duration=26, height=1.0, colour='#d8d2cc', intensity=0.5, start=4),
        ]))
    A['broadside_fire'] = add('juggernaut/broadside-fire', effect(
        'effect.beetles.broadside-fire', 'Broadside salvo', 'Both cannons roar at once.', 30, [
            flash('flash', FIRE, FIRE_LIGHT, 10.0, 10.0, duration=8, height=3.0),
            muzzle('muzzle', '#ffcf70', 5.0, duration=5, height=3.0, anchor=None),
            steam_burst('steam', 6.0, duration=24, height=2.4),
            shock('ring', FIRE_LIGHT, 8.0, 14, intensity=1.8),
            glint('glint', EMBER, 4.5, duration=6, height=3.0),
            streaks('sparks', EMBER, FIRE_LIGHT, count=20, speed=12, size=0.08, lifetime=14, height=3.0, gravity=12),
            smoke('smoke', '#55524f', '#9a9590', count=10, speed=3.0, size=1.8, height=3.0, lifetime=40, start=1,
                  rise=-1.0, spawn=0.5),
        ]))
    return A


def build_heroes():
    H = {}
    # Forgemaster
    H['turret'] = add('forgemaster/bolt-turret', effect(
        'effect.forgemaster.bolt-turret', 'Turret deploys',
        'The folded turret drops from the arm rig: a thump of steam, sparks and a brass flash.', 40, [
            column('column', SPARK, GOLD_LIGHT, 2.2, 12.0, 18, intensity=1.2),
            sigil('gear', GEAR, SPARK, 5.0, 40, spin=90, intensity=1.3),
            steam_burst('steam', 6.0, duration=26, height=0.8),
            puffs('puffs', count=10, speed=3.4, size=1.1, height=0.2, lifetime=34, rise=-0.9, cone=100,
                  direction=(0, 0.3, 0)),
            streaks('sparks', SPARK, SPARK_LIGHT, count=30, speed=10, size=0.09, lifetime=16, gravity=16, intensity=4.5),
            glint('glint', SPARK_LIGHT, 4.0, duration=7, height=1.0, intensity=3.0),
            flash('flash', SPARK, SPARK_LIGHT, 9.0, 8.0, duration=10, height=1.0),
            shock('ring', SPARK_LIGHT, 8.0, 14, intensity=2.0),
        ]))
    H['overclock'] = add('forgemaster/overclock', effect(
        'effect.forgemaster.overclock', 'Overclocked',
        'An overclocked unit runs hot: an orange glow, violet sparks thrown off and steam venting.', 80, [
            glow('heat', FIRE, FIRE_LIGHT, 2.4, 80, height=0.1, sustain=True, intensity=0.8,
                 motion={'opacity': pulse(10, 0.5, 1.0)}),
            sigil('gear', GEAR, FIRE, 1.8, 80, sustain=True, spin=60, intensity=0.8, follow=True),
            streaks('sparks', SPARK, SPARK_LIGHT, count=10, speed=5, size=0.07, lifetime=12, gravity=8, height=1.0,
                    spawn=0.6, cone=120, mode='continuous', duration=80, sustain=True, stretch=0.06, intensity=4.5),
            puffs('steam', count=3, speed=1.2, size=0.6, height=1.0, lifetime=24, rise=-1.8, mode='continuous',
                  duration=80).__or__({'sustain': True}),
            ring('ring', FIRE, FIRE_LIGHT, 1.8, 80, sustain=True, motion={'opacity': pulse(10, 0.4, 0.9)}),
        ], loop=True))
    H['overclock_on'] = add('forgemaster/overclock-burst', effect(
        'effect.forgemaster.overclock-burst', 'Overclock', 'A burst of heat as the governor is pulled.', 24, [
            column('column', FIRE, GOLD_LIGHT, 1.8, 9.0, 16, intensity=1.2),
            flash('flash', FIRE, FIRE_LIGHT, 7.0, 8.0, duration=10, height=1.0),
            glint('glint', GOLD_LIGHT, 3.4, duration=7, height=1.0, intensity=3.0),
            shock('ring', FIRE_LIGHT, 5.0, 12, intensity=1.8),
            streaks('sparks', EMBER, FIRE_LIGHT, count=26, speed=9, size=0.08, lifetime=14, height=1.0, gravity=14,
                    intensity=4.5),
            steam_burst('steam', 3.6, duration=22, height=0.8),
        ]))
    H['forge'] = add('forgemaster/field-forge', effect(
        'effect.forgemaster.field-forge', 'Field forge',
        'A warm forge ring around the Forgemaster with embers drifting up.', 80, [
            ring('ring', FIRE, FIRE_LIGHT, 9.5, 80, height=0.06, sustain=True, motion={'opacity': pulse(60, 0.25, 0.5)}),
            sigil('glyph', GEAR, SPARK, 3.4, 80, sustain=True, spin=480, intensity=0.6),
            layer('embers', 'particles', FIRE, FIRE_LIGHT, 80, 4, 0.12, 0.4, sustain=True, blend='additive',
                  texture=SPARK_DOT, intensity=3.0, core=0.7,
                  emitter=emitter('continuous', 30, 4, 1.4, gravity=-1.5, spawn=1.6, start=0.12, end=0.03, cone=25)),
        ], loop=True))
    H['forge_mend'] = add('forgemaster/field-forge-repair', effect(
        'effect.forgemaster.field-forge-repair', 'Forge repair', 'Small weld sparks where the field forge mends plating.', 20, [
            glint('glint', SPARK_LIGHT, 1.4, duration=5, height=1.2),
            streaks('sparks', SPARK, SPARK_LIGHT, count=10, speed=5, size=0.07, lifetime=10, height=1.2, gravity=16,
                    intensity=4.0),
        ]))
    # Forgefall: a red-hot anvil is called down, warned for 1.5 s, and falls in the last 0.6 s
    # to land on the `impact` event (the ability's delayTicks: 60), trailing fire, smoke and
    # a flame ribbon that stay in the air behind it (world-space particles riding its fall).
    # It lands, a blast wave of fire turning to ash races out over the ground, and the anvil
    # fades within a third of a second. Fire is a grey puff coloured through FIRE_RAMP: born
    # additive and hot, cooling down the ramp into dark alpha-blended ash, so it reads on sand.
    anvil_model = {'asset': 'asset.models.effects.beetles-anvil', 'role': 'geometry', 'index': 1,
                   'rotation': {'x': 0, 'y': 25, 'z': 0}}
    anvil_size = 3.4
    fall_start, fall = 36, 24
    H['forgefall_warn'] = add('forgemaster/forgefall-warning', effect(
        'effect.forgemaster.forgefall-warning', 'Forgefall warning',
        'A blazing warning circle and turning sigil while the anvil is in the air, its shadow darkening the '
        'ground; in the last 0.6 s the red-hot anvil itself drops out of the canopy, leaving a column of fire, '
        'smoke and embers hanging in the air behind it.',
        60, [
            ring('ring', '#ff5a2a', FIRE_LIGHT, 5.0, 60, sustain=True, motion={'opacity': pulse(10, 0.5, 1.0)}),
            ground('sigil', SIGIL_TEX, FIRE, FIRE_LIGHT, 10.0, 60, sustain=True,
                   motion={'rotation': {'periodTicks': 120, 'direction': 'clockwise'}, 'opacity': pulse(10, 0.45, 0.9)}),
            flash('glow', FIRE, FIRE_LIGHT, 8.0, 3.0, duration=60, height=1.0, sustain=True,
                  motion={'opacity': pulse(10, 0.4, 1.0)}),
            layer('shadow', 'glow', '#1a1210', '#000000', 30, 1, 4.2, 0.07, startTick=30, blend='normal',
                  opacityOverLife=keys((0, 0), (0.5, 0.35), (1, 0.7)), scaleOverLife=keys((0, 0.35), (1, 1.0))),
            layer('anvil', 'rain', WHITE, WHITE, fall, 1, anvil_size, 30, startTick=fall_start, spread=0.01,
                  fallSpeed=1, intensity=0.8, model=anvil_model),
            fire_trail('trail-fire', 'anvil', fall_start, fall),
            layer('trail-smoke', 'particles', '#3a3330', '#77706a', fall, 40, 1.0, 0, startTick=fall_start,
                  blend='normal', rides='anvil', offset={'x': 0, 'y': 1.6, 'z': 0}, texture=PUFF, flipbook=VARIANTS,
                  emitter=emitter('continuous', 40, 50, 1.2, variation=0.6, cone=180, drag=1.5, spawn=1.2,
                                  start=1.0, end=3.0) | {
                      'space': 'world', 'inheritVelocity': 0.05,
                      'opacityOverLife': keys((0, 0), (0.15, 0.6), (0.6, 0.4), (1, 0)),
                      'sizeOverLife': keys((0, 1.8), (1, 4.6)),
                      'erosionOverLife': keys((0, 0), (1, 0.6)),
                      'turbulence': {'strength': 0.6, 'scale': 2.0, 'speed': 1.0}}),
            layer('trail-ribbon', 'ribbon', '#fff0c0', '#a02008', fall, 1, 1.0, 0, startTick=fall_start,
                  blend='normal', rides='anvil', offset={'x': 0, 'y': 1.7, 'z': 0}, intensity=1.6,
                  texture=FLAME_STRIP,
                  ribbon={'durationTicks': 12, 'width': 3.4, 'tile': 5.0, 'scroll': 3.0,
                          'colourOverLife': [{'at': 0, 'colour': '#fff0c0'}, {'at': 0.3, 'colour': '#ffa040'},
                                             {'at': 1, 'colour': '#a02008'}],
                          'opacityOverLife': keys((0, 1), (1, 0)),
                          'widthOverLife': keys((0, 0.8), (0.2, 1.0), (1, 0.25)),
                          'additiveOverLife': keys((0, 0.6), (1, 0.9))}),
            layer('trail-embers', 'particles', EMBER, '#a02a08', fall, 40, 0.1, 0, startTick=fall_start,
                  blend='additive', rides='anvil', offset={'x': 0, 'y': 1.2, 'z': 0}, intensity=4.0, core=0.8,
                  texture=SPARK_DOT,
                  emitter=emitter('continuous', 16, 70, 5.0, variation=0.6, cone=180, drag=1.0, spawn=1.0,
                                  start=0.12, end=0.05, fade_out=0.5) | {'space': 'world', 'stretch': 0.04}),
            glow('heat', '#fff0c8', WHITE, 3.5, 24, height=0.08, startTick=36,
                 motion={'scale': pulse(48, 0.3, 1.2, 'smoothstep')}),
        ]))
    H['forgefall_hit'] = add('forgemaster/forgefall-impact', effect(
        'effect.forgemaster.forgefall-impact', 'Forgefall impact',
        'The anvil slams down and fades: a white flash, a blast wave of fire racing out over the ground and '
        'cooling into ash as it goes, streaks of flame, flung embers and rock, a sand-coloured dust ring, a '
        'scorched crater and a column of smoke.', 120, [
            flash('flash', '#ffb070', WHITE, 22.0, 16.0, duration=8, height=2.0),
            glint('flare', '#ffd9a0', 9.0, duration=4, height=1.4, art_=FLASH_ART, intensity=2.6),
            layer('anvil', 'mesh', WHITE, WHITE, 12, 1, anvil_size, 0.0, model=anvil_model, intensity=0.8,
                  opacityOverLife=keys((0, 1), (0.35, 0.85), (1, 0)), scaleOverLife=keys((0, 1), (1, 1.05))),
            layer('fireball', 'particles', WHITE, WHITE, 3, 36, 1.0, 0.8, texture=PUFF, flipbook=VARIANTS,
                  ramp=FIRE_RAMP, blend='additive',
                  emitter=emitter('burst', 34, 36, 7.0, variation=0.5, cone=70, drag=2.5, gravity=-3.0,
                                  spawn=1.5, start=2.0, end=5.0) | {
                      'sizeOverLife': keys((0, 1.6), (0.12, 4.2), (1, 6.0)),
                      'colourOverLife': [{'at': 0, 'colour': WHITE}, {'at': 0.45, 'colour': WHITE},
                                         {'at': 0.75, 'colour': '#5a463c'}, {'at': 1, 'colour': '#33302e'}],
                      'additiveOverLife': keys((0, 0.7), (0.45, 0.55), (0.75, 0)),
                      'opacityOverLife': keys((0, 0), (0.03, 1), (0.6, 0.8), (1, 0)),
                      'erosionOverLife': keys((0, 0), (0.5, 0.15), (1, 0.8)),
                      'heatOverLife': keys((0, 1.45), (0.4, 0.95), (1, 0.3)),
                      'turbulence': {'strength': 0.8, 'scale': 2.5, 'speed': 1.5}}),
            layer('blast', 'particles', WHITE, WHITE, 3, 180, 1.0, 0.6, texture=PUFF, flipbook=VARIANTS,
                  ramp=FIRE_RAMP, blend='additive',
                  emitter=emitter('burst', 44, 180, 34.0, variation=0.2, cone=88, drag=3.0, gravity=-0.8,
                                  spawn=1.4, start=1.0, end=3.0) | {
                      'coneInnerDegrees': 78, 'spawnInner': 0.4,
                      'sizeOverLife': keys((0, 1.2), (0.12, 3.0), (1, 5.5)),
                      'colourOverLife': [{'at': 0, 'colour': WHITE}, {'at': 0.28, 'colour': WHITE},
                                         {'at': 0.55, 'colour': '#6a5448'}, {'at': 1, 'colour': '#3a3532'}],
                      'additiveOverLife': keys((0, 0.7), (0.3, 0.55), (0.55, 0)),
                      'opacityOverLife': keys((0, 0), (0.03, 1), (0.5, 0.8), (1, 0)),
                      'erosionOverLife': keys((0, 0), (0.4, 0.1), (1, 0.8)),
                      'heatOverLife': keys((0, 1.3), (0.35, 0.8), (1, 0.3)),
                      'turbulence': {'strength': 0.6, 'scale': 2.0, 'speed': 2.0}}),
            layer('blast-streaks', 'particles', '#ffd27a', '#ff5a14', 3, 64, 0.18, 0.5, blend='additive',
                  intensity=4.0, core=0.8, texture=SPARK_DOT,
                  emitter=emitter('burst', 12, 64, 46.0, variation=0.4, cone=89, drag=2.5, spawn=0.8,
                                  start=0.28, end=0.12, fade_out=0.6) | {'coneInnerDegrees': 80, 'stretch': 0.05}),
            layer('ash', 'particles', '#2e2a28', '#3a3532', 3, 90, 0.22, 0.6, blend='normal', texture=ASH,
                  flipbook=VARIANTS,
                  emitter=emitter('burst', 60, 90, 18.0, variation=0.6, cone=85, gravity=7.0, drag=1.5, spawn=1.0,
                                  start=0.24, end=0.16, spin=6) | {
                      'coneInnerDegrees': 45, 'collide': {'friction': 0.85},
                      'opacityOverLife': keys((0, 0), (0.05, 1), (0.8, 1), (1, 0)),
                      'turbulence': {'strength': 0.5, 'scale': 1.5, 'speed': 1.5}}),
            layer('embers', 'particles', '#ffb04a', '#a02a08', 3, 40, 0.1, 0.6, blend='additive', intensity=4.0,
                  core=0.8, texture=SPARK_DOT,
                  emitter=emitter('burst', 40, 40, 14.0, variation=0.5, cone=70, gravity=9.0, drag=0.6, spawn=0.8,
                                  start=0.12, end=0.06, fade_out=0.4) | {'stretch': 0.03,
                                                                          'collide': {'friction': 0.6}}),
            layer('shock', 'billboard', '#ffcf8a', WHITE, 16, 1, 15.0, 0.1, orientation='ground', blend='additive',
                  intensity=1.6, core=0.4, texture=BLAST_RING,
                  scaleOverLife=keys((0, 0.15), (0.25, 0.7), (1, 1.0)), opacityOverLife=keys((0, 1), (0.5, 0.6), (1, 0))),
            layer('dust', 'particles', '#b89a72', '#9a8466', 3, 48, 1.0, 0.3, blend='normal', texture=PUFF,
                  flipbook=VARIANTS, soft=0.8,
                  emitter=emitter('burst', 56, 48, 20.0, variation=0.4, cone=89, drag=2.4, gravity=-0.3, spawn=2.0,
                                  start=1.0, end=3.6) | {
                      'coneInnerDegrees': 82, 'spawnInner': 0.6,
                      'sizeOverLife': keys((0, 1.6), (1, 5.5)),
                      'opacityOverLife': keys((0, 0), (0.1, 0.65), (1, 0)),
                      'erosionOverLife': keys((0, 0), (1, 0.7))}),
            layer('smoke', 'particles', '#2d2826', '#6e6862', 24, 64, 1.0, 0.6, startTick=3, blend='normal',
                  texture=PUFF, flipbook=VARIANTS,
                  emitter=emitter('continuous', 110, 40, 2.4, variation=0.5, cone=25, gravity=-2.2, drag=1.0,
                                  spawn=1.8, start=1.6, end=4.5) | {
                      'sizeOverLife': keys((0, 2.2), (1, 6.0)),
                      'opacityOverLife': keys((0, 0), (0.12, 0.75), (0.6, 0.5), (1, 0)),
                      'erosionOverLife': keys((0, 0.05), (1, 0.6)),
                      'turbulence': {'strength': 0.8, 'scale': 2.5, 'speed': 1.0}}),
            layer('debris', 'particles', '#7a6a58', '#6a5c4c', 3, 22, 0.35, 0.5, blend='normal', texture=CHUNK,
                  flipbook=VARIANTS,
                  emitter=emitter('burst', 50, 22, 14.0, variation=0.5, cone=50, gravity=22.0, drag=0.3, spawn=1.0,
                                  start=0.38, end=0.32, spin=8, fade_out=0.15) | {
                      'coneInnerDegrees': 25, 'collide': {'friction': 0.8}}),
            layer('scorch', 'billboard', WHITE, WHITE, 320, 1, 11.0, 0.05, startTick=1, orientation='ground',
                  blend='normal', intensity=0.9, texture=SCORCH,
                  opacityOverLife=keys((0, 0), (0.01, 1), (0.7, 0.95), (1, 0))),
        ]))
    H['forgefall_pool'] = add('forgemaster/forgefall-pool', effect(
        'effect.forgemaster.forgefall-pool', 'Molten pool',
        'A pool of molten metal for six seconds: hot veins glowing through a cooling crust, flames licking up '
        'from it, embers and smoke rising.', 240, [
            ground('crust', SCORCH, WHITE, WHITE, 10.0, 240, height=0.05, blend='normal', sustain=True),
            ground('pool', MOLTEN, WHITE, WHITE, 9.5, 240, height=0.07, sustain=True, intensity=1.6,
                   motion={'opacity': pulse(30, 0.75, 1.0)}),
            glow('heat', '#ff6a1a', FIRE_LIGHT, 5.0, 240, height=0.2, sustain=True, motion={'opacity': pulse(20, 0.35, 0.6)}),
            layer('licks', 'particles', WHITE, WHITE, 240, 40, 1.0, 0.2, sustain=True, blend='additive',
                  texture=PUFF, flipbook=VARIANTS, ramp=FIRE_RAMP,
                  emitter=emitter('continuous', 22, 22, 1.6, variation=0.5, cone=20, gravity=-2.0, drag=0.8,
                                  spawn=3.8, start=0.6, end=0.4) | {
                      'spawnInner': 0.1,
                      'sizeOverLife': keys((0, 0.5), (0.35, 1.1), (1, 0.4)),
                      'additiveOverLife': keys((0, 1), (0.7, 1), (1, 0.3)),
                      'opacityOverLife': keys((0, 0), (0.15, 0.9), (1, 0)),
                      'erosionOverLife': keys((0, 0.1), (1, 0.8)),
                      'heatOverLife': keys((0, 1.3), (1, 0.5)),
                      'turbulence': {'strength': 0.25, 'scale': 1.0, 'speed': 2.0}}),
            streaks('embers', EMBER, FIRE_LIGHT, count=10, speed=2.4, size=0.06, lifetime=30, gravity=-2.0, cone=20,
                    spawn=4.0, mode='continuous', duration=240, sustain=True, stretch=0.08, intensity=3.5),
            puffs('smoke', '#4a4644', '#8e8884', count=3, speed=0.8, size=1.6, height=0.4, lifetime=50, rise=-1.2,
                  mode='continuous', spawn=3.5, duration=240).__or__({'sustain': True}),
            flash('light', FIRE, FIRE_LIGHT, 9.0, 4.0, duration=240, height=0.8, sustain=True, motion={'opacity': pulse(16, 0.6, 1.0)}),
        ]))
    H['burn'] = add('forgemaster/molten-burn', effect(
        'effect.forgemaster.molten-burn', 'Molten burn', 'Flame licks at a unit standing in molten metal.', 18, [
            streaks('flames', FIRE, FIRE_LIGHT, count=6, speed=3, size=0.08, lifetime=14, gravity=-4, cone=25,
                    stretch=0.08),
        ]))
    # Coilkeeper
    H['lash'] = add('coilkeeper/arc-lash', effect(
        'effect.coilkeeper.arc-lash', 'Arc lash',
        'Violet lightning leaps from enemy to enemy: a thick crackling arc, a white core and a star on each.', 20, [
            *arcs('arc-', 1.4, 12, spread=1.1, forks=4),
            vl.lightning('fringe', '#9fb4ff', BLUE, 0.6, 7, spread=1.6, forks=2, height=1.3, intensity=1.2),
            bolt('bolt', '#f4f0ff', 8.0, duration=9, intensity=2.6),
            glint('glint', '#e6deff', 4.0, duration=7, height=1.0, intensity=3.2),
            substance_blast('burst', 14, 1.0, 5.0, lifetime=18, height=1.0),
            vl.ring('ring', VIOLET_LIGHT, 5.0, duration=10, intensity=1.4),
            streaks('sparks', count=24, speed=11, size=0.1, lifetime=14, height=1.0, gravity=6, cone=140,
                    intensity=4.5),
            flash('flash', VIOLET, VIOLET_LIGHT, 8.0, 9.0, duration=7, height=1.0),
        ]))
    H['overload'] = add('coilkeeper/overload', effect(
        'effect.coilkeeper.overload', 'Overload',
        "The target's mana caves in, then detonates in a violet blast: a collapsing ring, a white-violet star, "
        'a shock ring and a storm of hot streaks.', 36, [
            layer('implode', 'billboard', VIOLET_LIGHT, WHITE, 8, 1, 4.0, 0.12, orientation='ground',
                  blend='additive', intensity=1.8, core=0.6, texture=RING_ART,
                  motion={'scale': {'periodTicks': 16, 'min': 0.12, 'max': 1.0, 'easing': 'sine', 'phase': 0.5}}),
            orb('gather', VIOLET, VIOLET_LIGHT, 1.8, 8, height=1.2, intensity=1.2),
            layer('inflow', 'particles', VIOLET_LIGHT, VIOLET, 8, 30, 0.2, 1.2, blend='additive', texture=vl.MOTE,
                  intensity=2.6, core=0.8,
                  emitter=emitter('burst', 8, 30, 0.0, cone=180, spawn=3.5, start=0.2, end=0.1) | {
                      'spawnInner': 0.7, 'stretch': 0.04, 'inheritVelocity': 0.0,
                      'sizeOverLife': vl.keys((0, 0.1), (0.5, 0.25), (1, 0.05))}),
            sigil('glyph', ARC_GLYPH, VIOLET, 6.0, 30, spin=60, intensity=1.3),
            bolt('bolt', '#f4f0ff', 9.0, duration=10, start=6, intensity=2.6),
            column('pillar', VIOLET, '#efe8ff', 2.4, 14.0, 16, start=6, intensity=1.2),
            glint('blast', '#d8c8ff', 5.5, duration=8, height=1.2, start=6, intensity=3.0),
            substance_blast('fireball', 18, 1.3, 6.0, lifetime=24, height=1.4, start=6),
            substance_blast('wave', 90, 1.4, 26.0, lifetime=30, height=0.5, flat=True, start=6, spawn=0.8),
            layer('ring', 'billboard', VIOLET_LIGHT, WHITE, 16, 1, 12.0, 0.1, startTick=6, orientation='ground',
                  blend='additive', intensity=1.6, texture=vl.BLAST_RING,
                  scaleOverLife=vl.keys((0, 0.15), (0.25, 0.7), (1, 1.0)), opacityOverLife=vl.keys((0, 1), (1, 0))),
            streaks('sparks', count=40, speed=15, size=0.1, lifetime=18, height=1.0, start=6, gravity=6, cone=180,
                    intensity=4.5),
            flash('flash', VIOLET, VIOLET_LIGHT, 10.0, 10.0, duration=10, height=1.0, startTick=6),
        ]))
    H['overload_hit'] = add('coilkeeper/overload-shrapnel', effect(
        'effect.coilkeeper.overload-shrapnel', 'Overload shrapnel', 'Violet sparks on units caught in the detonation.', 16, [
            *arcs('arc-', 0.6, 6, spread=0.5, forks=1),
            bolt('bolt', VIOLET_LIGHT, 2.6, duration=5, intensity=1.6),
            glint('glint', '#d8ccff', 1.8, duration=5, height=1.0),
            streaks('sparks', count=10, speed=6, size=0.07, lifetime=10, height=1.0, gravity=4, cone=140, intensity=4.0),
        ]))
    H['recharge'] = add('coilkeeper/recharge', effect(
        'effect.coilkeeper.recharge', 'Recharge',
        'A column of violet light and a rising ring as substance flows back into allies.', 40, [
            column('pillar', VIOLET, '#efe8ff', 3.0, 14.0, 26, intensity=0.9),
            sigil('glyph', ARC_GLYPH, BLUE, 7.0, 40, spin=200, intensity=1.2),
            shock('ring', VIOLET_LIGHT, 12.0, 22, intensity=2.0),
            glow('glow', VIOLET, VIOLET_LIGHT, 6.0, 24, height=0.1, intensity=0.8),
            layer('motes', 'particles', VIOLET_LIGHT, WHITE, 40, 60, 0.3, 0.2, blend='additive', texture=vl.MOTE,
                  intensity=2.6, core=0.8,
                  emitter=emitter('burst', 40, 60, 3.0, gravity=-3.5, spawn=6.0, start=0.3, end=0.08, cone=20)
                  | {'spawnInner': 0.2, 'turbulence': {'strength': 0.6, 'scale': 1.5, 'speed': 2.0},
                     'opacityOverLife': vl.keys((0, 0), (0.1, 1), (0.7, 0.8), (1, 0))}),
            flash('flash', VIOLET, VIOLET_LIGHT, 9.0, 7.0, duration=10, height=1.0),
        ]))
    H['recharge_shield'] = add('coilkeeper/recharge-shield', effect(
        'effect.coilkeeper.recharge-shield', 'Charged shield', 'A violet shell crackling around a recharged ally.', 60, [
            orb('shell', VIOLET, VIOLET_LIGHT, 2.0, 60, height=1.1, sustain=True, intensity=0.5, core=0.25,
                motion={'opacity': pulse(20, 0.5, 1.0)}),
            ring('ring', VIOLET, VIOLET_LIGHT, 2.2, 60, sustain=True, intensity=0.7),
            streaks('crackle', '#9a78ff', SUBSTANCE_HOT, count=3, speed=2.5, size=0.05, lifetime=10, gravity=0,
                    height=1.1, spawn=0.9, cone=180, mode='continuous', duration=60, sustain=True, stretch=0.05,
                    intensity=3.0),
        ], loop=True))
    H['shatter'] = add('coilkeeper/unbottled-shatter', effect(
        'effect.coilkeeper.unbottled-shatter', 'The lantern shatters',
        'Glass and substance burst from the broken lantern: a blinding violet-white star, spinning glass shards, '
        'hot violet streaks and two shock rings.', 40, [
            flash('flash', '#e6dcff', WHITE, 20.0, 14.0, duration=12, height=2.0),
            column('pillar', VIOLET, WHITE, 3.2, 20.0, 24, intensity=1.4),
            bolt('bolt', '#efe8ff', 14.0, duration=10),
            glint('star', '#e6dcff', 9.0, duration=9, height=1.8, intensity=3.2),
            muzzle('burst', SUBSTANCE, 6.0, duration=6, height=1.8, anchor=None),
            shards('glass', count=36, speed=13, size=0.36, lifetime=40, height=1.6).__or__({}),
            substance_blast('fireball', 24, 1.8, 7.0, lifetime=28, height=2.0),
            substance_blast('wave', 140, 1.8, 34.0, lifetime=34, height=0.6, flat=True, spawn=1.2),
            streaks('streaks', count=48, speed=17, size=0.1, lifetime=20, height=1.6, gravity=8, cone=180,
                    intensity=4.5),
            layer('wave-ring', 'billboard', VIOLET_LIGHT, WHITE, 20, 1, 20.0, 0.1, orientation='ground',
                  blend='additive', intensity=1.8, texture=vl.BLAST_RING,
                  scaleOverLife=vl.keys((0, 0.1), (0.25, 0.7), (1, 1.0)), opacityOverLife=vl.keys((0, 1), (1, 0))),
            shock('wave2', VIOLET, 11.0, 26, start=6, height=0.14),
        ]))
    H['storm'] = add('coilkeeper/unbottled-storm', effect(
        'effect.coilkeeper.unbottled-storm', 'Unbottled storm',
        'A violet storm around the Coilkeeper for six seconds: a turning vortex on the ground, a swirling column and constant crackle.', 240, [
            ground('vortex', WHEEL_TEX, VIOLET, VIOLET_LIGHT, 22.0, 240, sustain=True, follow=True,
                   motion={'rotation': {'periodTicks': 60, 'direction': 'counterclockwise'}, 'opacity': pulse(20, 0.5, 0.9)}),
            sigil('inner', ARC_GLYPH, VIOLET_LIGHT, 9.0, 240, sustain=True, spin=40, intensity=1.2, follow=True),
            ring('edge', VIOLET, VIOLET_LIGHT, 14.0, 240, sustain=True, follow=True, motion={'opacity': pulse(12, 0.5, 1.0)}),
            column('column', VIOLET, VIOLET_LIGHT, 1.8, 14.0, 240, sustain=True, follow=True, intensity=0.25),
            streaks('swirl', VIOLET_LIGHT, WHITE, count=28, speed=4.5, size=0.1, lifetime=30, gravity=-5.0, cone=30,
                    spawn=12.0, mode='continuous', duration=240, sustain=True, follow=True, stretch=0.07,
                    intensity=3.5),
            layer('haze', 'particles', WHITE, WHITE, 240, 40, 2.0, 0.6, sustain=True, follow=True, blend='additive',
                  texture=vl.PUFF, flipbook=vl.VARIANTS, ramp=SUBSTANCE_RAMP,
                  emitter=emitter('continuous', 50, 30, 1.2, cone=40, gravity=-1.0, drag=1.0, spawn=11.0,
                                  start=2.0, end=3.0) | {
                      'spawnInner': 0.5,
                      'sizeOverLife': vl.keys((0, 1.2), (1, 3.2)),
                      'additiveOverLife': vl.keys((0, 0.8), (1, 0.4)),
                      'opacityOverLife': vl.keys((0, 0), (0.2, 0.55), (1, 0)),
                      'erosionOverLife': vl.keys((0, 0.1), (1, 0.75)),
                      'heatOverLife': vl.keys((0, 0.9), (1, 0.4)),
                      'turbulence': {'strength': 1.5, 'scale': 3.0, 'speed': 1.0}}),
            flash('light', VIOLET, VIOLET_LIGHT, 18.0, 6.0, duration=240, height=3.0, sustain=True, follow=True,
                  motion={'opacity': pulse(6, 0.5, 1.0)}),
        ]))
    H['storm_arc'] = add('coilkeeper/unbottled-arc', effect(
        'effect.coilkeeper.unbottled-arc', 'Storm arc', 'Lightning from the storm strikes a lifted enemy.', 12, [
            *arcs('arc-', 1.0, 7, height=2.4, spread=0.9, forks=3),
            bolt('bolt', '#e6deff', 4.0, duration=5),
            glint('glint', '#d8ccff', 2.6, duration=5, height=2.4, intensity=3.0),
            streaks('sparks', count=10, speed=7, size=0.07, lifetime=8, height=2.4, gravity=6, cone=180, intensity=4.0),
        ]))
    H['lifted'] = add('coilkeeper/lifted', effect(
        'effect.coilkeeper.lifted', 'Lifted', 'A violet glow and sparks around an enemy held up in the storm.', 40, [
            orb('glow', VIOLET, VIOLET_LIGHT, 1.4, 40, height=0.6, sustain=True, intensity=0.6,
                motion={'opacity': pulse(8, 0.5, 1.0)}),
            streaks('crackle', '#9a78ff', SUBSTANCE_HOT, count=4, speed=3, size=0.05, lifetime=10, gravity=0,
                    height=0.6, spawn=0.8, cone=180, mode='continuous', duration=40, sustain=True, stretch=0.05,
                    intensity=3.0),
        ], loop=True))
    H['sheltered'] = add('coilkeeper/unbottled-shelter', effect(
        'effect.coilkeeper.unbottled-shelter', 'Storm shelter', 'A bright blue bubble around allies the storm protects.', 60, [
            orb('bubble', BLUE, BLUE_LIGHT, 1.9, 60, height=1.2, sustain=True, intensity=0.4, core=0.2,
                motion={'opacity': pulse(24, 0.55, 1.0)}),
            ring('ring', BLUE, BLUE_LIGHT, 2.2, 60, sustain=True),
        ], loop=True))
    return H
