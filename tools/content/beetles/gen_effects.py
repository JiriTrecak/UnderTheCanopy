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

SUBSTANCE = '#8f66ff'  # violet sparks
SUBSTANCE_HOT = '#efe8ff'
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


def streaks(id_, colour=SUBSTANCE, accent=SUBSTANCE_HOT, count=12, speed=9.0, size=0.08, lifetime=12, gravity=12.0,
            cone=80, direction=(0, 1, 0), height=0.0, start=0, stretch=0.05, core=0.6, intensity=5.0, spawn=0.15,
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
            tracer('#8a5cff', WHITE, trail_colour='#7a4cff'),
        ]))
    W['bolt_hit'] = add('weapons/bolt-impact', effect(
        'effect.beetles.weapons.bolt-impact', 'Bolt impact',
        'A violet star and a spray of hot sparks where a bolt strikes.', 24, [
            glint('glint', SUBSTANCE, 2.2, duration=5, height=0.3, intensity=2.8),
            streaks('sparks', count=12, speed=8.5, size=0.08, lifetime=11, gravity=14, cone=80, height=0.3),
            puffs('dust', '#b8a890', '#e0d6c4', count=2, speed=1.2, size=0.55, height=0.0, lifetime=18, rise=-0.6),
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
            beam('halo', VIOLET, VIOLET_LIGHT, 1.1, 6, height=0.0, spread=0.45, intensity=0.35),
            beam('arc', '#9a74ff', '#efe8ff', 0.4, 8, height=0.0, spread=0.45),
            beam('core', '#f4f0ff', WHITE, 0.12, 6, height=0.0, spread=0.45),
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
            tracer('#8fa6ff', WHITE, size=0.13, length=7, width=0.06, trail=5, trail_colour=BLUE, intensity=4.0),
        ]))
    W['syringe_hit'] = add('weapons/syringe-impact', effect(
        'effect.beetles.weapons.syringe-impact', 'Dart splash', 'A small blue splash.', 18, [
            glint('glint', BLUE, 0.9, duration=4, height=0.3),
            streaks('drops', '#8fa6ff', '#dde6ff', count=7, speed=4, size=0.06, lifetime=12, gravity=14, height=0.3),
        ]))
    W['zap'] = add('weapons/prong-zap', effect(
        'effect.beetles.weapons.prong-zap', 'Prong discharge',
        'A short blue discharge between the projector prongs and the target.', 16, [
            beam('halo', BLUE, BLUE_LIGHT, 0.8, 5, height=0.0, spread=0.3, intensity=0.35),
            beam('arc', '#7f9bff', BLUE_LIGHT, 0.3, 6, height=0.0, spread=0.3),
            beam('core', '#eef2ff', WHITE, 0.1, 5, height=0.0, spread=0.3),
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
            tracer('#c4a8ff', WHITE, size=0.26, length=8, width=0.14, trail=6, trail_colour=VIOLET, intensity=5.0),
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
            tracer(EMBER, FIRE_LIGHT, size=0.45, length=1.6, width=0.32, trail=14, trail_colour='#ff7a2a', arc=9,
                   intensity=4.0),
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
            tracer(EMBER, FIRE_LIGHT, size=0.55, length=2.2, width=0.4, trail=12, trail_colour='#ff6a1a', arc=4.5,
                   intensity=4.5),
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
            ring('ring', BLUE, BLUE_LIGHT, 2.6, 80, sustain=True, motion={'opacity': pulse(40, 0.55, 0.95)}),
            glow('glow', BLUE, BLUE_LIGHT, 2.2, 80, height=0.1, sustain=True, motion={'scale': pulse(40, 0.9, 1.08)}),
            ground('plate', WARD_TEX, BLUE, BLUE_LIGHT, 3.2, 80, sustain=True, motion={'opacity': pulse(60, 0.35, 0.7)}),
        ], loop=True))
    A['brace_lock'] = add('shieldbearer/lock-shields-clang', effect(
        'effect.beetles.lock-shields-clang', 'Shields lock', 'Sparks and a vent of steam as the shield clamps lock.', 30, [
            glint('glint', BLUE, 1.6, duration=5, height=1.0),
            streaks('sparks', SPARK, SPARK_LIGHT, count=12, speed=7, size=0.07, lifetime=12, gravity=16, height=0.8),
            puffs('steam', count=6, speed=2.4, size=0.8, height=0.2, lifetime=26, rise=-1.0, cone=100,
                  direction=(0, 0.3, 0)),
            shock('ring', BLUE, 3.2, 10, intensity=1.2),
        ]))
    # Breacher
    A['crack'] = add('breacher/armor-crack', effect(
        'effect.beetles.armor-crack', 'Cracked armour',
        'Orange glints and a fracture glyph on armour the drill has opened.', 40, [
            ground('crack', FRACTURE_TEX, FIRE, FIRE_LIGHT, 2.2, 40, sustain=True, motion={'opacity': pulse(40, 0.4, 0.8)}),
            streaks('glints', FIRE, FIRE_LIGHT, count=3, speed=1.5, size=0.06, lifetime=14, gravity=6, height=1.2,
                    spawn=0.6, mode='continuous', duration=40, sustain=True, stretch=0.06, intensity=3.0),
        ], loop=True))
    # Arcwright
    A['surge_hop'] = add('arcwright/arc-surge-hop', effect(
        'effect.beetles.arc-surge-hop', 'Arc surge hop',
        'A thick violet arc that jumps from victim to victim, flashing on each.', 18, [
            beam('halo', VIOLET, VIOLET_LIGHT, 1.5, 8, height=1.2, spread=0.6, intensity=0.35),
            beam('arc', '#9a74ff', '#efe8ff', 0.55, 10, height=1.2, spread=0.6),
            beam('core', '#f8f4ff', WHITE, 0.16, 8, height=1.2, spread=0.6),
            glint('glint', SUBSTANCE, 2.6, duration=6, height=1.0),
            streaks('sparks', count=16, speed=9, size=0.07, lifetime=12, height=1.0, gravity=6, cone=140),
            flash('flash', VIOLET, VIOLET_LIGHT, 5.0, 7.0, duration=6, height=1.0),
        ]))
    A['static_zone'] = add('arcwright/static-field', effect(
        'effect.beetles.static-field', 'Static field',
        'A crackling violet field on the ground: a turning glyph, a bright rim and sparks that never stop.', 200, [
            ring('rim', VIOLET, VIOLET_LIGHT, 4.0, 200, sustain=True, motion={'opacity': pulse(20, 0.6, 1.0)}),
            ground('glyph', WHEEL_TEX, VIOLET, VIOLET_LIGHT, 7.6, 200, sustain=True,
                   motion={'rotation': {'periodTicks': 160, 'direction': 'clockwise'}, 'opacity': pulse(30, 0.45, 0.8)}),
            streaks('crackle', '#9a78ff', SUBSTANCE_HOT, count=14, speed=3.5, size=0.06, lifetime=10, gravity=4,
                    cone=80, spawn=3.6, mode='continuous', duration=200, sustain=True, stretch=0.05, height=0.2),
            flash('light', VIOLET, VIOLET_LIGHT, 7.0, 3.0, duration=200, height=1.0, sustain=True,
                  motion={'opacity': pulse(12, 0.5, 1.0)}),
        ]))
    A['static_hit'] = add('arcwright/static-shock', effect(
        'effect.beetles.static-shock', 'Static shock', 'A small violet spark on a unit the field bites.', 14, [
            glint('glint', SUBSTANCE, 1.0, duration=4, height=0.8),
            streaks('sparks', count=6, speed=5, size=0.05, lifetime=8, height=0.8, gravity=4, cone=140),
        ]))
    # Mender
    A['mend'] = add('mender/mend', effect(
        'effect.beetles.mend', 'Mend',
        'Blue-violet tonic glows up through the patient while white steam vents from the patched seams.', 40, [
            orb('glow', BLUE, BLUE_LIGHT, 1.6, 30, height=1.0, intensity=0.8),
            layer('motes', 'particles', '#9fb2ff', WHITE, 36, 14, 0.2, 0.3, blend='additive', texture=SPARK_DOT,
                  intensity=3.0, core=0.8, emitter=emitter('burst', 30, 14, 2.6, gravity=-3.0, spawn=0.7, start=0.2,
                                                         end=0.05, cone=30)),
            puffs('steam', count=4, speed=1.4, size=0.7, height=0.6, lifetime=28, rise=-1.8),
            flash('flash', BLUE, BLUE_LIGHT, 3.5, 4.0, duration=10, height=1.0),
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
            glint('glint', SPARK, 1.2, duration=5, height=1.0),
            streaks('sparks', SPARK, SPARK_LIGHT, count=12, speed=6.5, size=0.06, lifetime=12, gravity=16, height=1.0),
            puffs('steam', count=3, speed=1.2, size=0.7, height=0.8, lifetime=24, rise=-1.6),
        ]))
    # Warden
    A['barrier'] = add('warden/barrier-field', effect(
        'effect.beetles.barrier-field', 'Barrier field',
        'The violet pane of a field projector: a bright rim around the warden and a shimmering dome.', 80, [
            ring('rim', VIOLET, '#efe8ff', 4.4, 80, height=0.3, sustain=True, motion={'opacity': pulse(30, 0.6, 1.0)}),
            ring('inner', BLUE, BLUE_LIGHT, 3.6, 80, height=1.4, sustain=True, motion={'opacity': pulse(40, 0.25, 0.55)}),
            orb('dome', VIOLET, VIOLET_LIGHT, 3.4, 80, height=1.6, sustain=True, intensity=0.32, core=0.2,
                motion={'opacity': pulse(40, 0.5, 1.0)}),
            flash('light', VIOLET, VIOLET_LIGHT, 6.0, 2.0, duration=80, height=1.6, sustain=True),
        ], loop=True))
    A['barrier_up'] = add('warden/barrier-raise', effect(
        'effect.beetles.barrier-raise', 'Barrier raised', 'The projector flares as its pane comes up.', 24, [
            shock('wave', VIOLET_LIGHT, 5.0, 14),
            glint('glint', SUBSTANCE, 3.0, duration=6, height=1.4),
            streaks('sparks', count=12, speed=6, size=0.06, lifetime=12, height=1.2, gravity=4, cone=140),
            flash('flash', VIOLET, VIOLET_LIGHT, 6.0, 7.0, duration=8, height=1.2),
        ]))
    A['bulwark'] = add('warden/bulwark-pulse', effect(
        'effect.beetles.bulwark-pulse', 'Bulwark pulse', 'A blue shockwave of field energy rolling out from the warden.', 30, [
            shock('wave', BLUE, 9.0, 20, intensity=1.8),
            shock('wave2', VIOLET, 6.5, 16, start=4, height=0.4),
            glint('glint', BLUE_LIGHT, 3.6, duration=7, height=1.2),
            streaks('sparks', '#8fa6ff', BLUE_LIGHT, count=18, speed=9, size=0.07, lifetime=12, height=1.0, gravity=2,
                    cone=100, direction=(0, 0.2, 0)),
            flash('flash', BLUE, BLUE_LIGHT, 9.0, 8.0, duration=10, height=1.2),
        ]))
    A['ward'] = add('warden/bulwark-shield', effect(
        'effect.beetles.bulwark-shield', 'Field shield', 'A thin blue shell of field energy around a protected unit.', 60, [
            orb('shell', BLUE, BLUE_LIGHT, 1.7, 60, height=1.1, sustain=True, intensity=0.35, core=0.2,
                motion={'opacity': pulse(30, 0.5, 1.0)}),
            ground('ward', WARD_TEX, BLUE, BLUE_LIGHT, 2.4, 60, sustain=True, motion={'opacity': pulse(30, 0.4, 0.8)}),
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
        ]))
    A['broadside_fire'] = add('juggernaut/broadside-fire', effect(
        'effect.beetles.broadside-fire', 'Broadside salvo', 'Both cannons roar at once.', 30, [
            flash('flash', FIRE, FIRE_LIGHT, 10.0, 10.0, duration=8, height=3.0),
            muzzle('muzzle', '#ffcf70', 4.0, duration=5, height=3.0, anchor=None),
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
            puffs('steam', count=10, speed=3.4, size=1.1, height=0.2, lifetime=34, rise=-0.9, cone=100,
                  direction=(0, 0.3, 0)),
            streaks('sparks', SPARK, SPARK_LIGHT, count=22, speed=9, size=0.07, lifetime=16, gravity=16),
            glint('glint', SPARK, 2.4, duration=6, height=1.0),
            flash('flash', SPARK, SPARK_LIGHT, 6.0, 6.0, duration=8, height=1.0),
            shock('ring', SPARK, 3.6, 12),
        ]))
    H['overclock'] = add('forgemaster/overclock', effect(
        'effect.forgemaster.overclock', 'Overclocked',
        'An overclocked unit runs hot: an orange glow, violet sparks thrown off and steam venting.', 80, [
            glow('heat', FIRE, FIRE_LIGHT, 1.8, 80, height=0.1, sustain=True, motion={'opacity': pulse(10, 0.5, 1.0)}),
            streaks('sparks', VIOLET, VIOLET_LIGHT, count=6, speed=4.5, size=0.05, lifetime=12, gravity=8, height=1.0,
                    spawn=0.6, cone=120, mode='continuous', duration=80, sustain=True, stretch=0.05, intensity=3.5),
            puffs('steam', count=3, speed=1.2, size=0.6, height=1.0, lifetime=24, rise=-1.8, mode='continuous',
                  duration=80).__or__({'sustain': True}),
            ring('ring', FIRE, FIRE_LIGHT, 1.8, 80, sustain=True, motion={'opacity': pulse(10, 0.4, 0.9)}),
        ], loop=True))
    H['overclock_on'] = add('forgemaster/overclock-burst', effect(
        'effect.forgemaster.overclock-burst', 'Overclock', 'A burst of heat as the governor is pulled.', 24, [
            flash('flash', FIRE, FIRE_LIGHT, 5.0, 6.0, duration=8, height=1.0),
            glint('glint', EMBER, 2.2, duration=6, height=1.0),
            streaks('sparks', EMBER, FIRE_LIGHT, count=18, speed=8, size=0.07, lifetime=14, height=1.0, gravity=14),
            puffs('steam', count=6, speed=2.6, size=0.8, height=0.4, lifetime=26, rise=-1.0, cone=110),
        ]))
    H['forge'] = add('forgemaster/field-forge', effect(
        'effect.forgemaster.field-forge', 'Field forge',
        'A warm forge ring around the Forgemaster with embers drifting up.', 80, [
            ring('ring', FIRE, FIRE_LIGHT, 9.5, 80, height=0.06, sustain=True, motion={'opacity': pulse(60, 0.25, 0.5)}),
            ground('glyph', WHEEL_TEX, FIRE, FIRE_LIGHT, 3.6, 80, sustain=True,
                   motion={'rotation': {'periodTicks': 320, 'direction': 'counterclockwise'}, 'opacity': pulse(60, 0.3, 0.6)}),
            layer('embers', 'particles', FIRE, FIRE_LIGHT, 80, 4, 0.12, 0.4, sustain=True, blend='additive',
                  texture=SPARK_DOT, intensity=3.0, core=0.7,
                  emitter=emitter('continuous', 30, 4, 1.4, gravity=-1.5, spawn=1.6, start=0.12, end=0.03, cone=25)),
        ], loop=True))
    H['forge_mend'] = add('forgemaster/field-forge-repair', effect(
        'effect.forgemaster.field-forge-repair', 'Forge repair', 'Small weld sparks where the field forge mends plating.', 20, [
            glint('glint', SPARK, 0.9, duration=4, height=1.2),
            streaks('sparks', SPARK, SPARK_LIGHT, count=8, speed=5, size=0.05, lifetime=10, height=1.2, gravity=16),
        ]))
    # Forgefall: the crucible is called down, warned for 1.5 s, and falls in the last 0.75 s
    # to land on the `impact` event (the ability's delayTicks: 60).
    H['forgefall_warn'] = add('forgemaster/forgefall-warning', effect(
        'effect.forgemaster.forgefall-warning', 'Forgefall warning',
        'A blazing warning circle and turning sigil while the crucible is in the air; in the last three quarters of '
        'a second the iron crucible itself falls out of the canopy, brimming with molten metal and trailing fire.',
        60, [
            ring('ring', '#ff5a2a', FIRE_LIGHT, 5.0, 60, sustain=True, motion={'opacity': pulse(10, 0.5, 1.0)}),
            ground('sigil', SIGIL_TEX, FIRE, FIRE_LIGHT, 10.0, 60, sustain=True,
                   motion={'rotation': {'periodTicks': 120, 'direction': 'clockwise'}, 'opacity': pulse(10, 0.45, 0.9)}),
            flash('glow', FIRE, FIRE_LIGHT, 8.0, 3.0, duration=60, height=1.0, sustain=True,
                  motion={'opacity': pulse(10, 0.4, 1.0)}),
            layer('crucible', 'rain', WHITE, WHITE, 30, 1, 2.4, 26, startTick=30, spread=0.01, fallSpeed=1,
                  fallAngleDegrees=12, fallAzimuthDegrees=200,
                  model={'asset': 'asset.models.effects.beetles-crucible', 'role': 'geometry', 'index': 1,
                         'rotation': {'x': 0, 'y': 0, 'z': 0}}),
            layer('comet', 'rain', '#ff8a2a', FIRE_LIGHT, 30, 1, 2.6, 26, startTick=30, spread=0.01, fallSpeed=1,
                  fallAngleDegrees=12, fallAzimuthDegrees=200, length=4.2, blend='additive', intensity=2.4, core=0.9,
                  texture=SPARK_DOT, offset={'x': 0, 'y': 2.2, 'z': 0}),
            layer('embers', 'rain', EMBER, FIRE_LIGHT, 30, 14, 0.18, 26, startTick=30, spread=1.6, fallSpeed=1,
                  launchDelayMs=260, fallAngleDegrees=12, fallAzimuthDegrees=200, length=6, blend='additive',
                  intensity=3.0, core=0.8, texture=SPARK_DOT, offset={'x': 0, 'y': 1.0, 'z': 0}),
            glow('heat', '#fff0c8', WHITE, 3.5, 24, height=0.08, startTick=36,
                 motion={'scale': pulse(48, 0.3, 1.2, 'smoothstep')}),
        ]))
    H['forgefall_hit'] = add('forgemaster/forgefall-impact', effect(
        'effect.forgemaster.forgefall-impact', 'Forgefall impact',
        'The crucible slams down: a white-orange fireball, two shock rings, molten metal splashing out in hot '
        'streaks, a scorched crater and a pillar of smoke.', 240, [
            explosion('blast', 9.0, height=2.0),
            explosion('second', 6.5, height=1.2, start=5, offset={'x': 1.5, 'y': 0, 'z': -1.0}),
            flash('flash', '#ffb070', WHITE, 18.0, 12.0, duration=12, height=2.0),
            glint('glint', '#ffd9a0', 8.0, duration=7, height=1.4),
            shock('shock', FIRE_LIGHT, 13.0, 16, intensity=2.0),
            shock('shock2', FIRE, 9.0, 22, start=4, height=0.12),
            layer('splash', 'rain', '#ff9a3c', FIRE_LIGHT, 14, 16, 0.22, 0.6, spread=5.0, fallSpeed=1.5,
                  launchDelayMs=150, length=3, blend='additive', intensity=2.6, core=0.9, texture=SPARK_DOT,
                  impact={'durationTicks': 14, 'count': 5, 'size': 0.16, 'spread': 0.9, 'height': 0.6,
                          'colour': '#ff8a2a', 'accent': '#ffd9a0'}),
            streaks('molten', EMBER, FIRE_LIGHT, count=56, speed=15, size=0.11, lifetime=26, gravity=22, cone=70,
                    intensity=4.5, height=0.4),
            puffs('debris', '#3a3430', '#5a524c', count=14, speed=9, size=0.55, height=0.6, lifetime=34, rise=20,
                  cone=60, spawn=1.0),
            smoke('smoke', '#4a4644', '#8e8884', count=14, speed=3.0, size=2.6, height=0.6, lifetime=60, start=4,
                  spawn=2.0, cone=60, rise=-1.4),
            scorch('scorch', 11.0, duration=320, start=3),
        ]))
    H['forgefall_pool'] = add('forgemaster/forgefall-pool', effect(
        'effect.forgemaster.forgefall-pool', 'Molten pool',
        'A pool of molten metal for six seconds: hot veins glowing through a cooling crust, embers and smoke rising '
        'from it.', 240, [
            ground('crust', SCORCH, WHITE, WHITE, 10.0, 240, height=0.05, blend='normal', sustain=True),
            ground('pool', MOLTEN, WHITE, WHITE, 9.5, 240, height=0.07, sustain=True, intensity=1.6,
                   motion={'opacity': pulse(30, 0.75, 1.0)}),
            glow('heat', '#ff6a1a', FIRE_LIGHT, 5.0, 240, height=0.2, sustain=True, motion={'opacity': pulse(20, 0.35, 0.6)}),
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
            beam('halo', VIOLET, VIOLET_LIGHT, 1.7, 8, height=1.2, spread=0.7, intensity=0.35),
            beam('arc', '#9a74ff', '#efe8ff', 0.6, 12, height=1.2, spread=0.7),
            beam('core', '#faf8ff', WHITE, 0.18, 10, height=1.2, spread=0.7),
            beam('fringe', BLUE, BLUE_LIGHT, 0.3, 6, height=1.3, spread=1.0),
            glint('glint', SUBSTANCE, 3.0, duration=6, height=1.0),
            streaks('sparks', count=20, speed=10, size=0.07, lifetime=14, height=1.0, gravity=6, cone=140),
            flash('flash', VIOLET, VIOLET_LIGHT, 7.0, 8.0, duration=7, height=1.0),
        ]))
    H['overload'] = add('coilkeeper/overload', effect(
        'effect.coilkeeper.overload', 'Overload',
        "The target's mana caves in, then detonates in a violet blast: a collapsing ring, a white-violet star, "
        'a shock ring and a storm of hot streaks.', 36, [
            layer('implode', 'billboard', VIOLET_LIGHT, WHITE, 8, 1, 4.0, 0.12, orientation='ground',
                  blend='additive', intensity=1.8, core=0.6, texture=RING_ART,
                  motion={'scale': {'periodTicks': 16, 'min': 0.12, 'max': 1.0, 'easing': 'sine', 'phase': 0.5}}),
            orb('gather', VIOLET, VIOLET_LIGHT, 1.2, 8, height=1.2, intensity=0.9),
            glint('blast', '#d8c8ff', 5.5, duration=8, height=1.2, start=6, intensity=3.0),
            muzzle('burst', SUBSTANCE, 4.0, duration=5, height=1.2, anchor=None, start=6),
            shock('wave', VIOLET_LIGHT, 9.0, 16, start=6, intensity=2.0),
            streaks('sparks', count=40, speed=15, size=0.08, lifetime=18, height=1.0, start=6, gravity=6, cone=180,
                    intensity=4.5),
            flash('flash', VIOLET, VIOLET_LIGHT, 10.0, 10.0, duration=10, height=1.0, startTick=6),
        ]))
    H['overload_hit'] = add('coilkeeper/overload-shrapnel', effect(
        'effect.coilkeeper.overload-shrapnel', 'Overload shrapnel', 'Violet sparks on units caught in the detonation.', 16, [
            glint('glint', SUBSTANCE, 1.2, duration=4, height=1.0),
            streaks('sparks', count=8, speed=6, size=0.06, lifetime=10, height=1.0, gravity=4, cone=140),
        ]))
    H['recharge'] = add('coilkeeper/recharge', effect(
        'effect.coilkeeper.recharge', 'Recharge',
        'A column of violet light and a rising ring as substance flows back into allies.', 40, [
            layer('pillar', 'pillar', VIOLET, VIOLET_LIGHT, 24, 1, 6.0, 10, blend='additive', intensity=0.3),
            shock('ring', VIOLET_LIGHT, 7.0, 22),
            glow('glow', VIOLET, VIOLET_LIGHT, 5.0, 24, height=0.1),
            layer('motes', 'particles', VIOLET_LIGHT, WHITE, 40, 24, 0.16, 0.2, blend='additive', texture=SPARK_DOT,
                  intensity=3.5, core=0.8, distribution='disc', spread=5.0,
                  emitter=emitter('burst', 34, 24, 3.0, gravity=-3.0, spawn=5.0, start=0.16, end=0.04, cone=20)
                  | {'stretch': 0.06}),
            flash('flash', VIOLET, VIOLET_LIGHT, 9.0, 7.0, duration=10, height=1.0),
        ]))
    H['recharge_shield'] = add('coilkeeper/recharge-shield', effect(
        'effect.coilkeeper.recharge-shield', 'Charged shield', 'A violet shell crackling around a recharged ally.', 60, [
            orb('shell', VIOLET, VIOLET_LIGHT, 1.7, 60, height=1.1, sustain=True, intensity=0.35, core=0.2,
                motion={'opacity': pulse(20, 0.5, 1.0)}),
            streaks('crackle', '#9a78ff', SUBSTANCE_HOT, count=3, speed=2.5, size=0.05, lifetime=10, gravity=0,
                    height=1.1, spawn=0.9, cone=180, mode='continuous', duration=60, sustain=True, stretch=0.05,
                    intensity=3.0),
        ], loop=True))
    H['shatter'] = add('coilkeeper/unbottled-shatter', effect(
        'effect.coilkeeper.unbottled-shatter', 'The lantern shatters',
        'Glass and substance burst from the broken lantern: a blinding violet-white star, spinning glass shards, '
        'hot violet streaks and two shock rings.', 40, [
            flash('flash', '#e6dcff', WHITE, 20.0, 14.0, duration=12, height=2.0),
            glint('star', '#e6dcff', 9.0, duration=9, height=1.8, intensity=3.2),
            muzzle('burst', SUBSTANCE, 6.0, duration=6, height=1.8, anchor=None),
            shards('glass', count=36, speed=13, size=0.36, lifetime=24, height=1.6),
            streaks('streaks', count=48, speed=17, size=0.09, lifetime=20, height=1.6, gravity=8, cone=180,
                    intensity=4.5),
            shock('wave', VIOLET_LIGHT, 16.0, 20, intensity=2.2),
            shock('wave2', VIOLET, 11.0, 26, start=6, height=0.14),
        ]))
    H['storm'] = add('coilkeeper/unbottled-storm', effect(
        'effect.coilkeeper.unbottled-storm', 'Unbottled storm',
        'A violet storm around the Coilkeeper for six seconds: a turning vortex on the ground, a swirling column and constant crackle.', 240, [
            ground('vortex', WHEEL_TEX, VIOLET, VIOLET_LIGHT, 22.0, 240, sustain=True, follow=True,
                   motion={'rotation': {'periodTicks': 60, 'direction': 'counterclockwise'}, 'opacity': pulse(20, 0.5, 0.9)}),
            ground('inner', SIGIL_TEX, VIOLET_LIGHT, WHITE, 12.0, 240, sustain=True, follow=True,
                   motion={'rotation': {'periodTicks': 40, 'direction': 'clockwise'}, 'opacity': pulse(16, 0.4, 0.8)}),
            ring('edge', VIOLET, VIOLET_LIGHT, 14.0, 240, sustain=True, follow=True, motion={'opacity': pulse(12, 0.5, 1.0)}),
            layer('column', 'pillar', VIOLET, VIOLET_LIGHT, 240, 1, 4.0, 16, sustain=True, follow=True, blend='additive',
                  intensity=0.22),
            streaks('swirl', VIOLET_LIGHT, WHITE, count=28, speed=4.5, size=0.08, lifetime=30, gravity=-5.0, cone=30,
                    spawn=12.0, mode='continuous', duration=240, sustain=True, follow=True, stretch=0.07,
                    intensity=3.5),
            flash('light', VIOLET, VIOLET_LIGHT, 18.0, 6.0, duration=240, height=3.0, sustain=True, follow=True,
                  motion={'opacity': pulse(6, 0.5, 1.0)}),
        ]))
    H['storm_arc'] = add('coilkeeper/unbottled-arc', effect(
        'effect.coilkeeper.unbottled-arc', 'Storm arc', 'Lightning from the storm strikes a lifted enemy.', 12, [
            beam('halo', VIOLET, VIOLET_LIGHT, 1.3, 5, height=2.4, spread=0.6, intensity=0.35),
            beam('arc', '#9a74ff', '#efe8ff', 0.45, 6, height=2.4, spread=0.6),
            beam('core', '#faf8ff', WHITE, 0.12, 5, height=2.4),
            glint('glint', SUBSTANCE, 2.0, duration=5, height=2.4),
            streaks('sparks', count=8, speed=7, size=0.06, lifetime=8, height=2.4, gravity=6, cone=180),
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
