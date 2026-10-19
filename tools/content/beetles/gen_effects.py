"""The beetles' visual effects: substance blue-violet, fire orange, steam white (D65)."""
from gen_common import *

E = {}


def add(rel, e):
    E[e['id']] = e
    write('effects/beetles/' + rel + '.json', e)
    return e['id']


def missile_model(asset, size=1.0, arc=0.0):
    return layer('body', 'missile', WHITE, WHITE, 40, 1, size, 0, arcHeight=arc,
                 model={'asset': asset, 'role': 'geometry', 'index': 1, 'rotation': {'x': 0, 'y': 0, 'z': 0}})


def tracer(colour, accent, size=0.55, width=0.38, trail=12):
    return layer('tracer', 'missile', colour, accent, 40, 1, size, 0, blend='additive', texture=SPARK_TEX,
                 intensity=4.0,
                 trail={'durationTicks': trail, 'maxPoints': 12, 'width': width, 'colour': colour, 'opacity': 0.95,
                        'breakDistance': 6})


def build():
    # ----- weapons -------------------------------------------------------------------------
    W = {}
    W['bolt'] = add('weapons/bolt', effect(
        'effect.beetles.weapons.bolt', 'Pressure bolt', 'A brass bolt from a pressure repeater.', 40,
        [missile_model('asset.models.effects.amber-bolt', 0.8)]))
    W['muzzle'] = add('weapons/bolter-muzzle', effect(
        'effect.beetles.weapons.bolter-muzzle', 'Repeater volley',
        'Muzzle flash, steam vented from the canister and a violet-tinted tracer on every shot.', 40, [
            flash('flash', '#d8c8ff', WHITE, 6.0, 8.0, duration=5, height=0.2, anchor='source'),
            burst('muzzle', '#e6dcff', WHITE, 8, 0.9, 6, height=0.2, anchor='source'),
            sparks('sparks', '#b89cff', '#f0e8ff', count=10, speed=5.0, size=0.22, lifetime=12, gravity=6,
                   cone=50).__or__({'anchor': 'source'}),
            puffs('steam', count=5, speed=1.4, size=1.0, height=0.1, lifetime=28, rise=-2.0).__or__(
                {'anchor': 'source'}),
            tracer('#9d7bff', '#e4dbff'),
        ]))
    W['bolt_hit'] = add('weapons/bolt-impact', effect(
        'effect.beetles.weapons.bolt-impact', 'Bolt impact', 'Sparks and a small burst where a bolt strikes.', 24, [
            sparks('sparks', count=12, speed=6, size=0.2, lifetime=14),
            burst('pop', '#c9b8ff', WHITE, 5, 0.7, 6, height=0.0),
            flash('flash', '#b89cff', '#e8e0ff', 3.0, 4.0, duration=4, height=0.3),
            puffs('dust', '#b8a890', '#e0d6c4', count=3, speed=1.2, size=0.6, height=0.0, lifetime=20, rise=-0.6),
        ]))
    W['drill'] = add('weapons/drill-strike', effect(
        'effect.beetles.weapons.drill-strike', 'Drill strike',
        'Orange drill sparks and a burst of grit where the siege drill bites.', 30, [
            sparks('sparks', count=26, speed=8.5, size=0.26, lifetime=18, gravity=16, cone=95, intensity=3.5),
            flash('flash', FIRE, FIRE_LIGHT, 4.0, 5.0, duration=5, height=0.4),
            puffs('grit', '#a49076', '#d8c8ac', count=7, speed=2.6, size=0.85, height=0.0, lifetime=26, rise=-0.4,
                  cone=85),
        ]))
    W['bash'] = add('weapons/shield-bash', effect(
        'effect.beetles.weapons.shield-bash', 'Piston hammer blow', 'A clang of sparks and a puff of steam from the hammer.',
        26, [
            sparks('sparks', count=14, speed=6.5, size=0.22, lifetime=14, gravity=14, cone=90),
            puffs('steam', count=3, speed=1.0, size=0.7, height=0.3, lifetime=22, rise=-1.6),
            flash('flash', SPARK, SPARK_LIGHT, 2.6, 3.0, duration=4, height=0.4),
        ]))
    W['arc'] = add('weapons/arc', effect(
        'effect.beetles.weapons.arc', 'Substance arc',
        'A crackling violet arc from the coil to its target, with a flash at the coil.', 20, [
            beam('arc', VIOLET, '#efe8ff', 0.55, 8, height=0.0, spread=0.45),
            beam('core', '#f4f0ff', WHITE, 0.2, 6, height=0.0, spread=0.45),
            flash('flash', VIOLET, VIOLET_LIGHT, 5.0, 6.0, duration=5, height=0.2, anchor='source'),
            burst('coil', VIOLET_LIGHT, WHITE, 6, 0.6, 6, height=0.1, anchor='source'),
        ]))
    W['arc_hit'] = add('weapons/arc-impact', effect(
        'effect.beetles.weapons.arc-impact', 'Arc impact', 'Violet sparks crawl over whatever the arc strikes.', 24, [
            sparks('sparks', '#9a78ff', '#efe8ff', count=16, speed=6, size=0.22, lifetime=14, gravity=4, cone=120),
            burst('pop', VIOLET_LIGHT, WHITE, 6, 0.9, 6, height=0.0),
            flash('flash', VIOLET, VIOLET_LIGHT, 3.5, 5.0, duration=5, height=0.4),
        ]))
    W['syringe'] = add('weapons/syringe', effect(
        'effect.beetles.weapons.syringe', 'Syringe dart', 'A hissing dart of pressurised tonic.', 40, [
            puffs('hiss', count=2, speed=0.8, size=0.45, height=0.1, lifetime=18, rise=-1.4).__or__({'anchor': 'source'}),
            tracer('#7f9bff', '#dde6ff', size=0.25, width=0.12, trail=8),
        ]))
    W['syringe_hit'] = add('weapons/syringe-impact', effect(
        'effect.beetles.weapons.syringe-impact', 'Dart splash', 'A small blue splash.', 18, [
            burst('splash', '#7f9bff', '#dde6ff', 6, 0.6, 8, height=0.0),
            sparks('drops', '#7f9bff', '#dde6ff', count=6, speed=3.5, size=0.16, lifetime=12, gravity=10),
        ]))
    W['zap'] = add('weapons/prong-zap', effect(
        'effect.beetles.weapons.prong-zap', 'Prong discharge', 'A short blue discharge between the projector prongs and the target.', 16, [
            beam('arc', BLUE, BLUE_LIGHT, 0.4, 6, height=0.0, spread=0.3),
            flash('flash', BLUE, BLUE_LIGHT, 3.0, 4.0, duration=4, height=0.2, anchor='source'),
        ]))
    return W


def build_abilities():
    A = {}
    # Shieldbearer
    A['brace'] = add('shieldbearer/lock-shields', effect(
        'effect.beetles.lock-shields', 'Locked shields',
        'A blue-violet ring and a steady glow around a shieldbearer bracing behind a locked tower shield.', 80, [
            ring('ring', BLUE, BLUE_LIGHT, 2.6, 80, sustain=True, motion={'opacity': pulse(40, 0.55, 0.95)}),
            glow('glow', BLUE, BLUE_LIGHT, 2.2, 80, height=1.2, sustain=True, motion={'scale': pulse(40, 0.9, 1.08)}),
            ground('plate', WARD_TEX, BLUE, BLUE_LIGHT, 3.2, 80, sustain=True, motion={'opacity': pulse(60, 0.35, 0.7)}),
        ], loop=True))
    A['brace_lock'] = add('shieldbearer/lock-shields-clang', effect(
        'effect.beetles.lock-shields-clang', 'Shields lock', 'Sparks and a vent of steam as the shield clamps lock.', 30, [
            sparks('sparks', count=14, speed=5, size=0.2, lifetime=14),
            puffs('steam', count=6, speed=2.4, size=0.8, height=0.2, lifetime=26, rise=-1.0, cone=100,
                  direction=(0, 0.3, 0)),
            flash('flash', BLUE, BLUE_LIGHT, 4.0, 5.0, duration=6, height=0.8),
        ]))
    # Breacher
    A['crack'] = add('breacher/armor-crack', effect(
        'effect.beetles.armor-crack', 'Cracked armour',
        'Orange glints and a fracture glyph on armour the drill has opened.', 40, [
            ground('crack', FRACTURE_TEX, FIRE, FIRE_LIGHT, 2.2, 40, sustain=True, motion={'opacity': pulse(40, 0.4, 0.8)}),
            layer('glints', 'particles', FIRE, FIRE_LIGHT, 40, 3, 0.18, 1.2, sustain=True, blend='additive',
                  texture=SPARK_TEX, intensity=2.5,
                  emitter=emitter('continuous', 14, 3, 1.5, gravity=6, spawn=0.6, start=0.18, end=0.03)),
        ], loop=True))
    # Arcwright
    A['surge_hop'] = add('arcwright/arc-surge-hop', effect(
        'effect.beetles.arc-surge-hop', 'Arc surge hop',
        'A thick violet arc that jumps from victim to victim, flashing on each.', 18, [
            beam('arc', VIOLET, '#efe8ff', 0.7, 10, height=1.2, spread=0.6),
            beam('core', '#f8f4ff', WHITE, 0.25, 8, height=1.2, spread=0.6),
            burst('burst', VIOLET_LIGHT, WHITE, 8, 1.2, 8, height=1.0),
            sparks('sparks', '#9a78ff', '#efe8ff', count=14, speed=6, size=0.24, lifetime=14, height=1.0),
            flash('flash', VIOLET, VIOLET_LIGHT, 5.0, 7.0, duration=6, height=1.0),
        ]))
    A['static_zone'] = add('arcwright/static-field', effect(
        'effect.beetles.static-field', 'Static field',
        'A crackling violet field on the ground: a turning glyph, a bright rim and sparks that never stop.', 200, [
            ring('rim', VIOLET, VIOLET_LIGHT, 4.0, 200, sustain=True, motion={'opacity': pulse(20, 0.6, 1.0)}),
            ground('glyph', WHEEL_TEX, VIOLET, VIOLET_LIGHT, 7.6, 200, sustain=True,
                   motion={'rotation': {'periodTicks': 160, 'direction': 'clockwise'}, 'opacity': pulse(30, 0.45, 0.8)}),
            layer('crackle', 'particles', '#9a78ff', '#efe8ff', 200, 12, 0.22, 0.2, sustain=True, blend='additive',
                  texture=SPARK_TEX, intensity=3.0, distribution='disc', spread=3.6,
                  emitter=emitter('continuous', 10, 12, 3.0, gravity=4, spawn=3.6, start=0.22, end=0.04, cone=80)),
            flash('light', VIOLET, VIOLET_LIGHT, 7.0, 3.0, duration=200, height=1.0, sustain=True,
                  motion={'opacity': pulse(12, 0.5, 1.0)}),
        ]))
    A['static_hit'] = add('arcwright/static-shock', effect(
        'effect.beetles.static-shock', 'Static shock', 'A small violet spark on a unit the field bites.', 14, [
            sparks('sparks', '#9a78ff', '#efe8ff', count=6, speed=3.5, size=0.18, lifetime=10, height=0.8),
            burst('pop', VIOLET_LIGHT, WHITE, 3, 0.5, 5, height=0.8),
        ]))
    # Mender
    A['mend'] = add('mender/mend', effect(
        'effect.beetles.mend', 'Mend',
        'Blue-violet tonic glows up through the patient while white steam vents from the patched seams.', 40, [
            glow('glow', BLUE, BLUE_LIGHT, 2.0, 30, height=1.0),
            layer('motes', 'particles', '#9fb2ff', WHITE, 36, 14, 0.24, 0.3, blend='additive', texture=SPARK_TEX,
                  intensity=2.5, emitter=emitter('burst', 30, 14, 2.6, gravity=-3.0, spawn=0.7, start=0.24, end=0.05,
                                                 cone=30)),
            puffs('steam', count=4, speed=1.4, size=0.7, height=0.6, lifetime=28, rise=-1.8),
            flash('flash', BLUE, BLUE_LIGHT, 3.5, 4.0, duration=10, height=1.0),
        ]))
    A['weld'] = add('mender/field-repair', effect(
        'effect.beetles.field-repair', 'Welding torch',
        'A white-hot weld point spraying orange sparks over the plating being repaired.', 40, [
            glow('point', '#fff4d8', WHITE, 0.8, 40, height=1.2),
            layer('spray', 'particles', SPARK, SPARK_LIGHT, 40, 20, 0.2, 1.2, blend='additive', texture=SPARK_TEX,
                  intensity=3.5, emitter=emitter('continuous', 14, 20, 6.5, gravity=16, cone=70, start=0.2, end=0.03)),
            flash('flash', '#fff0d0', WHITE, 4.0, 4.0, duration=40, height=1.2, motion={'opacity': pulse(4, 0.4, 1.0)}),
        ]))
    A['patch'] = add('tinker/patch-up', effect(
        'effect.beetles.patch-up', 'Patched up', 'A clatter of sparks and a puff of steam as a machine is patched.', 28, [
            sparks('sparks', count=12, speed=5, size=0.2, lifetime=14, height=1.0),
            puffs('steam', count=3, speed=1.2, size=0.7, height=0.8, lifetime=24, rise=-1.6),
            flash('flash', SPARK, SPARK_LIGHT, 3.0, 3.0, duration=6, height=1.0),
        ]))
    # Warden
    A['barrier'] = add('warden/barrier-field', effect(
        'effect.beetles.barrier-field', 'Barrier field',
        'The violet pane of a field projector: a bright rim around the warden and a shimmering dome.', 80, [
            ring('rim', VIOLET, '#efe8ff', 4.4, 80, height=0.3, sustain=True, motion={'opacity': pulse(30, 0.6, 1.0)}),
            ring('inner', BLUE, BLUE_LIGHT, 3.6, 80, height=1.4, sustain=True, motion={'opacity': pulse(40, 0.25, 0.55)}),
            glow('dome', VIOLET, VIOLET_LIGHT, 4.2, 80, height=1.4, sustain=True, motion={'opacity': pulse(40, 0.3, 0.6)}),
            flash('light', VIOLET, VIOLET_LIGHT, 6.0, 2.0, duration=80, height=1.6, sustain=True),
        ], loop=True))
    A['barrier_up'] = add('warden/barrier-raise', effect(
        'effect.beetles.barrier-raise', 'Barrier raised', 'The projector flares as its pane comes up.', 24, [
            ring('wave', VIOLET_LIGHT, WHITE, 4.5, 16, height=0.4),
            flash('flash', VIOLET, VIOLET_LIGHT, 6.0, 7.0, duration=8, height=1.2),
            sparks('sparks', '#9a78ff', '#efe8ff', count=12, speed=4, size=0.2, lifetime=14, height=1.2),
        ]))
    A['bulwark'] = add('warden/bulwark-pulse', effect(
        'effect.beetles.bulwark-pulse', 'Bulwark pulse', 'A blue shockwave of field energy rolling out from the warden.', 30, [
            ring('wave', BLUE, BLUE_LIGHT, 8.0, 22, height=0.3),
            ring('wave2', VIOLET, VIOLET_LIGHT, 6.0, 18, height=0.6, startTick=4),
            flash('flash', BLUE, BLUE_LIGHT, 9.0, 8.0, duration=10, height=1.2),
            burst('burst', BLUE_LIGHT, WHITE, 10, 1.8, 10, height=1.2),
        ]))
    A['ward'] = add('warden/bulwark-shield', effect(
        'effect.beetles.bulwark-shield', 'Field shield', 'A thin blue shell of field energy around a protected unit.', 60, [
            glow('shell', BLUE, BLUE_LIGHT, 2.0, 60, height=1.1, sustain=True, motion={'opacity': pulse(30, 0.35, 0.7)}),
            ground('ward', WARD_TEX, BLUE, BLUE_LIGHT, 2.4, 60, sustain=True, motion={'opacity': pulse(30, 0.4, 0.8)}),
        ], loop=True))
    # Juggernaut
    A['broadside'] = add('juggernaut/broadside', effect(
        'effect.beetles.broadside', 'Broadside',
        'A rolling barrage: three fireballs walk across the target area with sparks and smoke.', 60, [
            explosion('blast1', 5.5, height=1.2),
            explosion('blast2', 5.0, height=1.2, start=6, offset={'x': 2.2, 'y': 0, 'z': 1.4}),
            explosion('blast3', 5.0, height=1.2, start=12, offset={'x': -2.0, 'y': 0, 'z': -1.6}),
            flash('flash', FIRE, FIRE_LIGHT, 12.0, 9.0, duration=18, height=1.0),
            sparks('sparks', count=40, speed=11, size=0.3, lifetime=22, gravity=18, cone=85),
            puffs('smoke', '#5d5a58', '#a09c98', count=10, speed=2.2, size=2.2, height=0.4, lifetime=50, rise=-0.8,
                  cone=90, start=4, spawn=2.5),
            ring('shock', FIRE_LIGHT, WHITE, 6.0, 18),
        ]))
    A['broadside_fire'] = add('juggernaut/broadside-fire', effect(
        'effect.beetles.broadside-fire', 'Broadside salvo', 'Both cannons roar at once.', 30, [
            flash('flash', FIRE, FIRE_LIGHT, 10.0, 10.0, duration=8, height=3.0),
            burst('muzzle', FIRE, FIRE_LIGHT, 12, 2.0, 8, height=3.0),
            puffs('smoke', '#55524f', '#9a9590', count=10, speed=3.0, size=1.8, height=3.0, lifetime=40, rise=-1.0),
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
            sparks('sparks', count=22, speed=7, size=0.24, lifetime=16),
            flash('flash', SPARK, SPARK_LIGHT, 6.0, 6.0, duration=8, height=1.0),
            ring('ring', SPARK, SPARK_LIGHT, 3.0, 14),
        ]))
    H['overclock'] = add('forgemaster/overclock', effect(
        'effect.forgemaster.overclock', 'Overclocked',
        'An overclocked unit runs hot: an orange glow, violet sparks thrown off and steam venting.', 80, [
            glow('heat', FIRE, FIRE_LIGHT, 1.8, 80, height=1.0, sustain=True, motion={'opacity': pulse(10, 0.5, 1.0)}),
            layer('sparks', 'particles', VIOLET, VIOLET_LIGHT, 80, 6, 0.2, 1.0, sustain=True, blend='additive',
                  texture=SPARK_TEX, intensity=3.0,
                  emitter=emitter('continuous', 12, 6, 3.5, gravity=8, spawn=0.6, start=0.2, end=0.03, cone=120)),
            puffs('steam', count=3, speed=1.2, size=0.6, height=1.0, lifetime=24, rise=-1.8, mode='continuous',
                  duration=80).__or__({'sustain': True}),
            ring('ring', FIRE, FIRE_LIGHT, 1.8, 80, sustain=True, motion={'opacity': pulse(10, 0.4, 0.9)}),
        ], loop=True))
    H['overclock_on'] = add('forgemaster/overclock-burst', effect(
        'effect.forgemaster.overclock-burst', 'Overclock', 'A burst of heat as the governor is pulled.', 24, [
            flash('flash', FIRE, FIRE_LIGHT, 5.0, 6.0, duration=8, height=1.0),
            sparks('sparks', count=18, speed=6, size=0.22, lifetime=14, height=1.0),
            puffs('steam', count=6, speed=2.6, size=0.8, height=0.4, lifetime=26, rise=-1.0, cone=110),
        ]))
    H['forge'] = add('forgemaster/field-forge', effect(
        'effect.forgemaster.field-forge', 'Field forge',
        'A warm forge ring around the Forgemaster with embers drifting up.', 80, [
            ring('ring', FIRE, FIRE_LIGHT, 9.5, 80, height=0.06, sustain=True, motion={'opacity': pulse(60, 0.25, 0.5)}),
            ground('glyph', WHEEL_TEX, FIRE, FIRE_LIGHT, 3.6, 80, sustain=True,
                   motion={'rotation': {'periodTicks': 320, 'direction': 'counterclockwise'}, 'opacity': pulse(60, 0.3, 0.6)}),
            layer('embers', 'particles', FIRE, FIRE_LIGHT, 80, 4, 0.16, 0.4, sustain=True, blend='additive',
                  texture=SPARK_TEX, intensity=2.5,
                  emitter=emitter('continuous', 30, 4, 1.4, gravity=-1.5, spawn=1.6, start=0.16, end=0.03, cone=25)),
        ], loop=True))
    H['forge_mend'] = add('forgemaster/field-forge-repair', effect(
        'effect.forgemaster.field-forge-repair', 'Forge repair', 'Small weld sparks where the field forge mends plating.', 20, [
            sparks('sparks', count=8, speed=4, size=0.18, lifetime=12, height=1.2),
            flash('flash', SPARK, SPARK_LIGHT, 2.5, 2.0, duration=5, height=1.2),
        ]))
    H['forgefall_warn'] = add('forgemaster/forgefall-warning', effect(
        'effect.forgemaster.forgefall-warning', 'Forgefall warning',
        'A blazing warning circle and sigil on the ground while the crucible is in the air; it falls in the last half second.', 60, [
            ring('ring', '#ff5a2a', FIRE_LIGHT, 5.0, 60, sustain=True, motion={'opacity': pulse(10, 0.5, 1.0)}),
            ground('sigil', SIGIL_TEX, FIRE, FIRE_LIGHT, 10.0, 60, sustain=True,
                   motion={'rotation': {'periodTicks': 120, 'direction': 'clockwise'}, 'opacity': pulse(10, 0.45, 0.9)}),
            flash('glow', FIRE, FIRE_LIGHT, 8.0, 3.0, duration=60, height=1.0, sustain=True, motion={'opacity': pulse(10, 0.4, 1.0)}),
            # The crucible's glow comes down in the last half second: a column of fire from
            # the canopy and a growing white-hot spot where it will strike.
            layer('crucible', 'pillar', FIRE, FIRE_LIGHT, 24, 1, 2.6, 28, startTick=36, blend='additive'),
            glow('heat', '#fff0c8', WHITE, 3.5, 24, height=0.4, startTick=36,
                 motion={'scale': pulse(24, 0.4, 1.4, 'smoothstep')}),
        ]))
    H['forgefall_hit'] = add('forgemaster/forgefall-impact', effect(
        'effect.forgemaster.forgefall-impact', 'Forgefall impact',
        'The crucible slams down: a white-orange fireball, a shockwave, molten splashes and a pillar of smoke.', 60, [
            explosion('blast', 9.0, height=2.0),
            explosion('second', 6.5, height=1.2, start=5, offset={'x': 1.5, 'y': 0, 'z': -1.0}),
            flash('flash', '#ffb070', WHITE, 18.0, 12.0, duration=12, height=2.0),
            sparks('splash', FIRE, FIRE_LIGHT, count=60, speed=13, size=0.36, lifetime=26, gravity=20, cone=75, intensity=4.0),
            ring('shock', FIRE_LIGHT, WHITE, 7.0, 18),
            ring('shock2', FIRE, FIRE_LIGHT, 5.0, 24, startTick=4),
            layer('pillar', 'pillar', FIRE, FIRE_LIGHT, 14, 1, 2.6, 14, blend='additive'),
            puffs('smoke', '#4a4644', '#8e8884', count=14, speed=3.0, size=2.6, height=0.6, lifetime=60, rise=-1.4,
                  cone=60, start=4, spawn=2.0),
        ]))
    H['forgefall_pool'] = add('forgemaster/forgefall-pool', effect(
        'effect.forgemaster.forgefall-pool', 'Molten pool',
        'A pool of molten metal glowing orange for six seconds, embers and heat shimmer rising from it.', 240, [
            ground('pool', FRACTURE_TEX, '#ff6a1a', FIRE_LIGHT, 9.5, 240, sustain=True, motion={'opacity': pulse(30, 0.7, 1.0)}),
            glow('heat', '#ff6a1a', FIRE_LIGHT, 5.0, 240, height=0.2, sustain=True, motion={'opacity': pulse(20, 0.5, 0.9)}),
            ring('rim', FIRE, FIRE_LIGHT, 5.0, 240, sustain=True, motion={'opacity': pulse(24, 0.5, 0.9)}),
            layer('embers', 'particles', FIRE, FIRE_LIGHT, 240, 10, 0.22, 0.2, sustain=True, blend='additive',
                  texture=SPARK_TEX, intensity=3.0, distribution='disc', spread=4.5,
                  emitter=emitter('continuous', 30, 10, 2.0, gravity=-2.0, spawn=4.5, start=0.22, end=0.04, cone=20)),
            puffs('smoke', '#4a4644', '#8e8884', count=3, speed=0.8, size=1.6, height=0.4, lifetime=50, rise=-1.2,
                  mode='continuous', spawn=3.5, duration=240).__or__({'sustain': True}),
            flash('light', FIRE, FIRE_LIGHT, 9.0, 4.0, duration=240, height=0.8, sustain=True, motion={'opacity': pulse(16, 0.6, 1.0)}),
        ]))
    H['burn'] = add('forgemaster/molten-burn', effect(
        'effect.forgemaster.molten-burn', 'Molten burn', 'Flame licks at a unit standing in molten metal.', 18, [
            sparks('flames', FIRE, FIRE_LIGHT, count=6, speed=2.5, size=0.3, lifetime=14, gravity=-4, cone=25),
        ]))
    # Coilkeeper
    H['lash'] = add('coilkeeper/arc-lash', effect(
        'effect.coilkeeper.arc-lash', 'Arc lash',
        'Violet lightning leaps from enemy to enemy: a thick crackling arc, a white core and a burst on each.', 20, [
            beam('arc', VIOLET, '#efe8ff', 0.8, 12, height=1.2, spread=0.7),
            beam('core', '#faf8ff', WHITE, 0.3, 10, height=1.2, spread=0.7),
            beam('fringe', BLUE, BLUE_LIGHT, 0.9, 6, height=1.3),
            burst('burst', VIOLET_LIGHT, WHITE, 10, 1.6, 9, height=1.0),
            sparks('sparks', '#9a78ff', '#efe8ff', count=18, speed=7, size=0.26, lifetime=16, height=1.0),
            flash('flash', VIOLET, VIOLET_LIGHT, 7.0, 8.0, duration=7, height=1.0),
        ]))
    H['overload'] = add('coilkeeper/overload', effect(
        'effect.coilkeeper.overload', 'Overload',
        "The target's mana caves in, then detonates in a violet blast.", 36, [
            glow('implode', VIOLET, VIOLET_LIGHT, 2.6, 10, height=1.2),
            burst('blast', VIOLET_LIGHT, WHITE, 14, 3.0, 12, height=1.0, startTick=6),
            ring('wave', VIOLET, VIOLET_LIGHT, 4.5, 16, startTick=6),
            sparks('sparks', '#9a78ff', '#efe8ff', count=30, speed=10, size=0.3, lifetime=20, height=1.0, start=6, cone=120),
            flash('flash', VIOLET, VIOLET_LIGHT, 10.0, 10.0, duration=10, height=1.0, startTick=6),
        ]))
    H['overload_hit'] = add('coilkeeper/overload-shrapnel', effect(
        'effect.coilkeeper.overload-shrapnel', 'Overload shrapnel', 'Violet sparks on units caught in the detonation.', 16, [
            sparks('sparks', '#9a78ff', '#efe8ff', count=8, speed=4, size=0.2, lifetime=12, height=1.0),
        ]))
    H['recharge'] = add('coilkeeper/recharge', effect(
        'effect.coilkeeper.recharge', 'Recharge',
        'A column of violet light and a rising ring as substance flows back into allies.', 40, [
            layer('pillar', 'pillar', VIOLET, VIOLET_LIGHT, 24, 1, 6.0, 10, blend='additive'),
            ring('ring', VIOLET_LIGHT, WHITE, 6.0, 26),
            glow('glow', VIOLET, VIOLET_LIGHT, 5.0, 24, height=0.6),
            layer('motes', 'particles', VIOLET_LIGHT, WHITE, 40, 24, 0.24, 0.2, blend='additive', texture=SPARK_TEX,
                  intensity=3.0, distribution='disc', spread=5.0,
                  emitter=emitter('burst', 34, 24, 3.0, gravity=-3.0, spawn=5.0, start=0.24, end=0.04, cone=20)),
            flash('flash', VIOLET, VIOLET_LIGHT, 9.0, 7.0, duration=10, height=1.0),
        ]))
    H['recharge_shield'] = add('coilkeeper/recharge-shield', effect(
        'effect.coilkeeper.recharge-shield', 'Charged shield', 'A violet shell crackling around a recharged ally.', 60, [
            glow('shell', VIOLET, VIOLET_LIGHT, 2.0, 60, height=1.1, sustain=True, motion={'opacity': pulse(20, 0.35, 0.75)}),
            layer('crackle', 'particles', '#9a78ff', '#efe8ff', 60, 3, 0.16, 1.1, sustain=True, blend='additive',
                  texture=SPARK_TEX, intensity=2.5,
                  emitter=emitter('continuous', 10, 3, 2.0, gravity=0, spawn=0.9, start=0.16, end=0.03, cone=180)),
        ], loop=True))
    H['shatter'] = add('coilkeeper/unbottled-shatter', effect(
        'effect.coilkeeper.unbottled-shatter', 'The lantern shatters',
        'Glass and substance burst from the broken lantern: a blinding violet-white flash and a ring of shards.', 40, [
            flash('flash', '#e6dcff', WHITE, 20.0, 14.0, duration=12, height=2.0),
            burst('glass', '#dfe8ff', WHITE, 24, 2.6, 12, height=1.6),
            sparks('shards', '#c9d8ff', WHITE, count=50, speed=14, size=0.3, lifetime=22, height=1.6, gravity=10, cone=180),
            ring('wave', VIOLET_LIGHT, WHITE, 14.0, 22),
            ring('wave2', VIOLET, VIOLET_LIGHT, 10.0, 28, startTick=6),
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
                  ),
            layer('swirl', 'particles', VIOLET_LIGHT, WHITE, 240, 24, 0.3, 0.3, sustain=True, follow=True, blend='additive',
                  texture=SPARK_TEX, intensity=3.5, distribution='disc', spread=12.0,
                  emitter=emitter('continuous', 30, 24, 3.5, gravity=-5.0, spawn=12.0, start=0.3, end=0.05, cone=30)),
            flash('light', VIOLET, VIOLET_LIGHT, 18.0, 6.0, duration=240, height=3.0, sustain=True, follow=True,
                  motion={'opacity': pulse(6, 0.5, 1.0)}),
        ]))
    H['storm_arc'] = add('coilkeeper/unbottled-arc', effect(
        'effect.coilkeeper.unbottled-arc', 'Storm arc', 'Lightning from the storm strikes a lifted enemy.', 12, [
            beam('arc', VIOLET, '#efe8ff', 0.6, 6, height=2.4, spread=0.6),
            beam('core', '#faf8ff', WHITE, 0.16, 5, height=2.4),
            burst('burst', VIOLET_LIGHT, WHITE, 6, 1.0, 6, height=2.4),
        ]))
    H['lifted'] = add('coilkeeper/lifted', effect(
        'effect.coilkeeper.lifted', 'Lifted', 'A violet glow and sparks around an enemy held up in the storm.', 40, [
            glow('glow', VIOLET, VIOLET_LIGHT, 1.6, 40, height=0.6, sustain=True, motion={'opacity': pulse(8, 0.5, 1.0)}),
            layer('crackle', 'particles', '#9a78ff', '#efe8ff', 40, 4, 0.18, 0.6, sustain=True, blend='additive',
                  texture=SPARK_TEX, intensity=3.0,
                  emitter=emitter('continuous', 10, 4, 2.5, gravity=0, spawn=0.8, start=0.18, end=0.03, cone=180)),
        ], loop=True))
    H['sheltered'] = add('coilkeeper/unbottled-shelter', effect(
        'effect.coilkeeper.unbottled-shelter', 'Storm shelter', 'A bright blue bubble around allies the storm protects.', 60, [
            glow('bubble', BLUE, BLUE_LIGHT, 2.4, 60, height=1.2, sustain=True, motion={'opacity': pulse(24, 0.45, 0.85)}),
            ring('ring', BLUE, BLUE_LIGHT, 2.2, 60, sustain=True),
        ], loop=True))
    return H
