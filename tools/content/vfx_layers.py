"""Shared effect layer builders for the content generators (every race): the trails that
ride projectiles and the shared grey textures (`vfx_art.py`). Plain dicts in the effect
format (skill canopy-abilities, presentation.md); colours are `#rrggbb`.

A weapon's launch effect flies the same path as its projectile: `carrier` is that path
(invisible, or a glowing head), and trails `ride` it in world space, so what they shed
stays in the air behind the shot.
"""


def tex(name):
    return {'asset': 'asset.effects.vfx.' + name, 'role': 'image', 'index': 1}


PUFF = tex('puff')
FLAME_STRIP = tex('flame-strip')
ASH = tex('ash')
CHUNK = tex('chunk')
BLAST_RING = tex('blast-ring')
MOTE = tex('mote')
WAKE = tex('wake')
BOLT = tex('bolt')
WHITE = '#ffffff'
# Four still variants of a 2 × 2 atlas, one picked per particle.
VARIANTS = {'columns': 2, 'rows': 2, 'firstFrame': 0, 'frames': 4, 'frameTicks': 400, 'mode': 'once',
            'randomStart': True}
# Texture brightness → fire colour: a dark ember rim, red, orange, a white-hot heart.
FIRE_RAMP = [{'at': 0.0, 'colour': '#3a1206'}, {'at': 0.25, 'colour': '#b8320a'}, {'at': 0.5, 'colour': '#ff7a1a'},
             {'at': 0.75, 'colour': '#ffc04a'}, {'at': 1.0, 'colour': '#fff4d0'}]
# Flight effects last as long as the flight (the runtime passes it); 40 ticks otherwise.
FLIGHT = 40


def keys(*pairs):
    """A curve over a life: (at, value) pairs."""
    return [{'at': at, 'value': value} for at, value in pairs]


def colours(*pairs):
    """A colour curve or ramp: (at, '#rrggbb') pairs."""
    return [{'at': at, 'colour': colour} for at, colour in pairs]


def layer(id_, shape, colour, accent, duration, count, size, height, **extra):
    l = {'id': id_, 'enabled': True, 'shape': shape, 'colour': colour, 'accent': accent,
         'durationTicks': duration, 'count': count, 'size': size, 'height': height}
    l.update(extra)
    return l


def emitter(mode, lifetime, rate, speed, *, variation=0.4, direction=(0, 1, 0), cone=60, gravity=0.0,
            drag=0.5, spawn=0.2, start=0.3, end=0.05, spin=0, fade_in=0.0, fade_out=0.5, **extra):
    e = {'mode': mode, 'lifetimeTicks': lifetime, 'rate': rate, 'speed': speed, 'speedVariation': variation,
         'direction': {'x': direction[0], 'y': direction[1], 'z': direction[2]}, 'coneDegrees': cone,
         'gravity': gravity, 'drag': drag, 'spawnRadius': spawn, 'startSize': start, 'endSize': end,
         'spin': spin, 'fadeIn': fade_in, 'fadeOut': fade_out}
    e.update(extra)
    return e


def carrier(arc=0.0, id_='path', head=None):
    """The flight's path: an invisible missile (or, with `head` = (colour, size, intensity),
    a glowing mote) along the projectile's arc that trails ride."""
    if head is None:
        return layer(id_, 'missile', WHITE, WHITE, FLIGHT, 1, 0.1, 0, arcHeight=arc, intensity=0.0)
    colour, size, intensity = head
    return layer(id_, 'missile', colour, WHITE, FLIGHT, 1, size, 0, arcHeight=arc, blend='additive',
                 intensity=intensity, core=0.85, texture=MOTE)


def wake(id_, rides, colour, accent=None, width=0.2, ticks=5, opacity=0.6, additive=0.0, texture=WAKE,
         tile=1.2, scroll=0.0):
    """A soft band streaming behind the flight (air for thorns, light for bolts). `additive`
    0–1: glowing tails partly cover the ground (about 0.6) so they stay saturated on sand."""
    ribbon = {'durationTicks': ticks, 'width': width, 'tile': tile, 'scroll': scroll,
              'opacityOverLife': keys((0, 1), (1, 0)),
              'widthOverLife': keys((0, 0.6), (0.15, 1), (1, 0.35))}
    additive = float(additive)
    if 0.0 < additive < 1.0:
        ribbon['additiveOverLife'] = keys((0, additive), (1, min(1.0, additive + 0.3)))
    return layer(id_, 'ribbon', colour, accent or colour, FLIGHT, 1, 1.0, 0, rides=rides,
                 blend='additive' if additive >= 1.0 else 'normal', texture=texture, intensity=opacity,
                 ribbon=ribbon)


def motes(id_, rides, colour, accent, rate, size, lifetime, *, additive=True, speed=0.8, spread=0.12, gravity=0.0,
          intensity=1.0, core=0.0, texture=MOTE, stretch=None, collide=None, inherit=0.1, turbulence=None,
          count=None):
    """Small particles shed along the flight that stay where they were shed."""
    e = emitter('continuous', lifetime, rate, speed, variation=0.6, cone=180, gravity=gravity, drag=1.5,
                spawn=spread, start=size, end=size * 0.25, fade_out=0.6, space='world', inheritVelocity=inherit)
    if stretch:
        e['stretch'] = stretch
    if collide is not None:
        e['collide'] = {'friction': collide}
    if turbulence:
        e['turbulence'] = turbulence
    alive = count or max(4, min(128, int(rate * lifetime / 40 + 2)))
    l = layer(id_, 'particles', colour, accent, FLIGHT, alive, size, 0, rides=rides,
              blend='additive' if additive else 'normal', intensity=intensity, texture=texture, emitter=e)
    if core:
        l['core'] = core
    return l


def drips(id_, rides, colour, accent, rate, size, *, lifetime=26, additive=0.0, intensity=1.0):
    """Droplets falling from the flight and landing (nectar, venom, water)."""
    e = emitter('continuous', lifetime, rate, 0.6, variation=0.6, cone=180, gravity=14.0, drag=0.4, spawn=0.08,
                start=size, end=size * 0.6, fade_out=0.3, space='world', inheritVelocity=0.35,
                collide={'friction': 0.9}, stretch=0.02,
                additiveOverLife=keys((0, additive), (1, additive * 0.5)))
    return layer(id_, 'particles', colour, accent, FLIGHT, max(4, min(96, int(rate * lifetime / 40 + 2))), size,
                 0, rides=rides, blend='normal', intensity=intensity, texture=MOTE, emitter=e)


def smoke_trail(id_, rides, colour, accent, rate, size, lifetime, *, opacity=0.55, rise=-0.6):
    """Smoke left hanging along the flight, swelling and drifting (shells, steam)."""
    e = emitter('continuous', lifetime, rate, 0.4, variation=0.6, cone=180, gravity=rise, drag=1.2, spawn=0.1,
                start=size, end=size * 3, space='world', inheritVelocity=0.05,
                sizeOverLife=keys((0, size * 0.6), (0.2, size * 1.2), (1, size * 3.0)),
                opacityOverLife=keys((0, 0), (0.1, opacity), (1, 0)),
                erosionOverLife=keys((0, 0), (1, 0.6)),
                turbulence={'strength': size * 0.5, 'scale': 1.5, 'speed': 1.0})
    return layer(id_, 'particles', colour, accent, FLIGHT, max(6, min(128, int(rate * lifetime / 40 + 2))), size,
                 0, rides=rides, blend='normal', texture=PUFF, flipbook=VARIANTS, emitter=e)


def fire_trail(id_, rides, rate, size, lifetime, *, ramp=None, heat=1.3):
    """Burning puffs shed along the flight, cooling down the ramp into smoke."""
    e = emitter('continuous', lifetime, rate, 0.6, variation=0.6, cone=180, drag=1.5, spawn=0.15, start=size,
                end=size * 2, space='world', inheritVelocity=0.1,
                sizeOverLife=keys((0, size * 0.7), (0.25, size * 1.3), (1, size * 2.2)),
                colourOverLife=colours((0, WHITE), (0.5, WHITE), (0.85, '#5a463c'), (1, '#33302e')),
                additiveOverLife=keys((0, 0.75), (0.5, 0.6), (0.85, 0)),
                opacityOverLife=keys((0, 0), (0.08, 1), (0.6, 0.7), (1, 0)),
                erosionOverLife=keys((0, 0), (1, 0.7)),
                heatOverLife=keys((0, heat), (1, 0.35)),
                turbulence={'strength': size * 0.3, 'scale': 1.2, 'speed': 1.5})
    return layer(id_, 'particles', WHITE, WHITE, FLIGHT, max(6, min(128, int(rate * lifetime / 40 + 2))), size, 0,
                 rides=rides, blend='additive', texture=PUFF, flipbook=VARIANTS, ramp=ramp or FIRE_RAMP, emitter=e)


def burst(id_, colour, accent, count, size, *, speed=6.0, lifetime=14, gravity=6.0, additive=True, height=0.6,
          intensity=1.6, core=0.6, texture=MOTE, cone=120, stretch=None, collide=None):
    """A burst of motes thrown out on impact."""
    e = emitter('burst', lifetime, count, speed, variation=0.5, cone=cone, gravity=gravity, drag=1.5, spawn=0.2,
                start=size, end=size * 0.2, fade_out=0.6)
    if stretch:
        e['stretch'] = stretch
    if collide is not None:
        e['collide'] = {'friction': collide}
    l = layer(id_, 'particles', colour, accent, lifetime + 2, count, size, height,
              blend='additive' if additive else 'normal', intensity=intensity, texture=texture, emitter=e)
    if core and additive:
        l['core'] = core
    return l


def flare(id_, colour, size, *, duration=5, height=0.6, intensity=2.0, texture=MOTE):
    """A quick soft flash at the impact."""
    return layer(id_, 'billboard', colour, WHITE, duration, 1, size, height, blend='additive', intensity=intensity,
                 core=0.8, texture=texture, scaleOverLife=keys((0, 0.5), (0.3, 1.0), (1, 1.2)),
                 opacityOverLife=keys((0, 1), (1, 0)))


def ring(id_, colour, size, *, duration=10, intensity=1.2, additive=True):
    """A thin ground ring popping out from the impact."""
    return layer(id_, 'billboard', colour, WHITE, duration, 1, size, 0.08, orientation='ground',
                 blend='additive' if additive else 'normal', intensity=intensity, texture=BLAST_RING,
                 scaleOverLife=keys((0, 0.2), (0.3, 0.75), (1, 1.0)), opacityOverLife=keys((0, 1), (1, 0)))


def puff(id_, colour, accent, count, size, *, speed=2.0, lifetime=24, height=0.3, opacity=0.5, rise=-0.5,
         cone=80):
    """Dust or smoke puffs kicked up on impact (alpha-blended)."""
    e = emitter('burst', lifetime, count, speed, variation=0.5, cone=cone, gravity=rise, drag=2.0, spawn=0.3,
                start=size, end=size * 2.4,
                sizeOverLife=keys((0, size * 0.6), (1, size * 2.4)),
                opacityOverLife=keys((0, 0), (0.1, opacity), (1, 0)),
                erosionOverLife=keys((0, 0), (1, 0.6)))
    return layer(id_, 'particles', colour, accent, lifetime + 2, count, size, height, blend='normal', texture=PUFF,
                 flipbook=VARIANTS, emitter=e)


def lightning(id_, colour, accent, width, duration, *, spread=0.6, forks=3, height=1.2, intensity=1.6,
              additive=True, **extra):
    """A crackling bolt between the cue's ends: a textured beam that wanders and forks."""
    l = layer(id_, 'beam', colour, accent, duration, 1, width, height, blend='additive' if additive else 'normal',
              texture=BOLT, spread=spread, forks=forks, intensity=intensity)
    l.update(extra)
    return l
