"""Writes the ant race's gameplay: unit and hero kits, abilities and their effects, research,
building behaviours, the build lists and the race (docs/ants.md is the design).

Run from anywhere: python3 tools/content/ants/gen.py (writes this repository's base/). Every
number lives here, so balance passes edit one place and regenerate. The ants keep their
definitions' old ids (`unit.ants.warrior` is the Raider; docs/ants.md lists them all) and their
models, portraits and presentation: this script patches the gameplay fields of each existing
definition and creates the new ones.
"""
import copy
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'beetles'))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ant_effects as antfx  # noqa: E402  (tools/content/ants/ant_effects.py)
from gen_common import (BASE, WRITTEN, write, remove, cost, rp, cast, self_aim, unit_aim, point_aim, anims, fx,
                        ability, passive, binding, layer, emitter, flash, ring, glow, burst, pulse, effect, puffs,
                        sparks, VIOLET, VIOLET_LIGHT, WHITE, DUST_TEX, SPARK_TEX)

S = 40  # ticks per second
ENGINE = os.environ.get('ANTS_ENGINE', '1') == '1'  # tunnels, broods and Found (engine features)

AMBER = '#ffb347'
AMBER_LIGHT = '#ffe2a8'
SCENT = '#b89cff'
SCENT_LIGHT = '#efe6ff'
ALARM = '#ff9a3c'
ALARM_LIGHT = '#ffe0b0'
FUNGUS = '#c8a8ff'
FUNGUS_LIGHT = '#f4ecff'
BLUE = '#5b7bff'
BLUE_LIGHT = '#c8d4ff'
CRIMSON = '#ff4f6a'


def u(name):
    return 'unit.ants.' + name


def b(name):
    return 'building.ants.' + name


def r(name):
    return 'research.ants.' + name


# The ants' definitions keep their first ids (docs/ants.md, Unit models).
WORKER, RAIDER, NEEDLER, MAJOR = u('settler'), u('warrior'), u('archer'), u('queensguard')
BURSTER, HONEYPOT, ALATE, BULLET = u('burster'), u('bombardier'), u('winglancer'), u('thornblade')
HERALD, GARDENER, REAVER = u('banneret'), u('nectarist'), u('reaver-queen')
QUEEN, ALCHEMIST, GENERAL = u('queen'), u('royal-alchemist'), u('marshal')
SPITTER = u('spire-spitter')
NEST, GREAT_NEST, CHAMBER = b('fort'), b('great-mound'), b('elder-hall')
MOUND, BURROW, WORKS = b('house'), b('barracks'), b('ironroot-forge')
GARDEN, SCENT_HALL, GALLERY = b('fungus-garden'), b('scent-hall'), b('bombardier-workshop')
CRADLE, SPIRE, VENOM_SPIRE = b('queens-cradle'), b('tower'), b('venom-spire')
ROYAL, SIPHON = b('sanctuary'), b('substance-well')
T2, T3 = GREAT_NEST, CHAMBER

# Small ants: what the Major guards and the Raiders' frenzy spreads to (level 1 and 2).
SMALL = {'heroes': False, 'maxLevel': 2}
PHYSICAL = ('melee', 'piercing', 'siege', 'hero')


def icon(name):
    return 'asset.icons.ability-ants-' + name


def ricon(name):
    return 'asset.icons.research-ants-' + name


def taken(permille, types=PHYSICAL + ('spell',)):
    return {t: permille for t in types}


FX = {}


def build_effects():
    shutil_rmtree(BASE + 'effects/ants/kit')
    FX.update(antfx.build())


def shutil_rmtree(path):
    import shutil
    if os.path.isdir(path):
        shutil.rmtree(path)


# ----- unit abilities -----------------------------------------------------------------------

def put_ability(rel, a):
    write('abilities/ants/' + rel + '.json', a)
    return a['id']


def unit_abilities():
    put_ability('raider/frenzy', passive(
        'ability.ants.frenzy', 'Frenzy',
        'Raiders work each other into a frenzy: every second a Raider fights beside other small ants, they gain 6% attack damage, up to 24%. It fades a moment after they part.',
        [{}], [], icon('raider-frenzy'),
        [fx('frenzy', FX['frenzy'], 'statusApplied', lifetime='status', statusId='frenzy')],
        triggers=[{'id': 'stir', 'event': 'interval', 'intervalTicks': S, 'blockedBySilence': False, 'operations': [
            {'op': 'status', 'target': 'target',
             'query': {'center': 'caster', 'radius': 5, 'relations': ['ally'], 'allowSelf': False, 'maxTargets': 12},
             'filter': SMALL, 'id': 'frenzy', 'amount': 60, 'polarity': 'positive', 'dispel': True,
             'stacking': {'max': 4, 'scope': 'ability'}, 'modifiers': {'damagePermille': 60}}]}]))

    put_ability('needler/barbed-needles', passive(
        'ability.ants.barbed-needles', 'Barbed Needles',
        'Barbed needles stay in the wound: each hit lowers the target\'s armor by 1 for 5 seconds, up to 3. Learned at the Fungus Garden.',
        [{'armor': -1}], [], icon('needler-barbed-needles'),
        [fx('barbed', FX['barbed'], 'statusApplied', lifetime='status', statusId='barbed')],
        combatModifiers={'attackBonus': {'status': 'barbed', 'amount': 0, 'manaCost': 0, 'weapon': 'projectile',
                                         'blockedBySilence': False}},
        statuses=[{'op': 'status', 'target': 'target', 'id': 'barbed', 'amount': 5 * S, 'polarity': 'negative',
                   'dispel': True, 'stacking': {'max': 3, 'scope': 'ability'},
                   'rankedModifiers': {'armor': rp('armor')}}]))

    burst_ops = [
        {'op': 'damage', 'target': 'target',
         'query': {'center': 'caster', 'radius': 4, 'relations': ['enemy'], 'includeBuildings': True, 'maxTargets': 24},
         'amount': rp('blast'), 'damageType': 'siege'},
        {'op': 'damage', 'target': 'target',
         'query': {'center': 'caster', 'radius': 4, 'relations': ['ally'], 'maxTargets': 24},
         'amount': rp('allies'), 'damageType': 'spell'}]
    put_ability('burster/unstable', passive(
        'ability.ants.unstable', 'Unstable',
        'Mutated. The Burster bursts on its first bite, and whenever it dies: 100 siege damage to enemies and buildings within 4 units, and 40 to its own allies there. Volatile Sacs raises the blast to 150 and spares allies.',
        [{'blast': 100, 'allies': 40}, {'blast': 150, 'allies': 0}], [], icon('burster-unstable'),
        [fx('burst', FX['burst'], 'death', anchor='target')],
        triggers=[{'id': 'bite', 'event': 'weaponRelease', 'blockedBySilence': False, 'operations': [
            {'op': 'damage', 'target': 'caster', 'amount': 100000, 'damageType': 'spell'}]},
                  {'id': 'burst', 'event': 'death', 'operations': [
                      dict(o, query=dict(o['query'], center='point')) for o in burst_ops]}]))

    put_ability('burster/rupture', ability(
        'ability.ants.rupture', 'Rupture',
        'Burst now, wherever the Burster stands (Unstable deals the damage).',
        'targeted', [{}], self_aim(), cast(4, 0, 0, 0),
        [{'op': 'damage', 'target': 'caster', 'amount': 100000, 'damageType': 'spell'}],
        icon('burster-rupture'), [], animations=anims('attack', 'attack', 'idle')))

    put_ability('major/guard', passive(
        'ability.ants.guard', 'Guard',
        'Small ants within 6 units of a Major take 15% less damage. Living Wall raises it to 25%.',
        [{'taken': 850}, {'taken': 750}],
        [{'op': 'status', 'target': 'target', 'id': 'guarded', 'amount': 1, 'polarity': 'positive', 'dispel': False,
          'damageTakenPermille': {t: rp('taken') for t in PHYSICAL}},
         {'op': 'status', 'target': 'caster', 'id': 'guarding', 'amount': 1, 'polarity': 'positive', 'dispel': False}],
        icon('major-guard'),
        [fx('guard', FX['guard'], 'statusApplied', lifetime='status', statusId='guarding'),
         fx('guarded', FX['guarded'], 'statusApplied', lifetime='status', statusId='guarded')],
        aura={'radius': 6, 'meleeOnly': False}) | {'targeting': self_aim() | {'filter': SMALL}})

    put_ability('major/plug', ability(
        'ability.ants.plug', 'Plug',
        'Toggle. The Major plants its huge head like a door: it cannot move, but gains 5 armor and takes half damage from missiles and 30% less from siege.',
        'targeted', [{}], self_aim(), cast(6, 4, 0, 40),
        [{'op': 'status', 'target': 'caster', 'id': 'plugged', 'amount': 80, 'polarity': 'positive', 'dispel': False,
          'lifetime': 'instance', 'modifiers': {'rooted': True, 'armor': 5},
          'damageTakenPermille': {'piercing': 500, 'siege': 700}}],
        icon('major-plug'),
        [fx('slam', FX['plug-slam'], 'statusApplied', statusId='plugged'),
         fx('plug', FX['plug'], 'statusApplied', lifetime='status', statusId='plugged')],
        persistent={'durationTicks': 144000, 'intervalTicks': 40, 'anchor': 'caster', 'endsWithCaster': True,
                    'upkeepMana': 0, 'toggle': True}))

    put_ability('bullet-ant/agony', ability(
        'ability.ants.agony', 'Agony',
        'The most painful sting in the forest: 40 spell damage and a 1-second stun, then for 6 seconds the target moves 30% slower and takes 25% more damage. Venom Glands makes it last 10 seconds.',
        'targeted', [{'linger': 6 * S}, {'linger': 10 * S}],
        unit_aim(['enemy'], 2, filter={'buildings': False}), cast(6, 8, 50, 12 * S),
        [{'op': 'damage', 'target': 'target', 'amount': 40, 'damageType': 'spell'},
         {'op': 'status', 'target': 'target', 'id': 'stung', 'amount': S, 'heroDuration': S // 2,
          'polarity': 'negative', 'dispel': True, 'stun': True},
         {'op': 'status', 'target': 'target', 'id': 'agony', 'amount': rp('linger'), 'polarity': 'negative',
          'dispel': True, 'modifiers': {'moveSpeedPermille': -300}, 'damageTakenPermille': taken(1250)}],
        icon('bullet-agony'),
        [fx('windup', FX['cast-substance'], 'accepted', anchor='caster'),
         fx('hit', FX['agony-hit'], 'damaged'),
         fx('agony', FX['agony'], 'statusApplied', lifetime='status', statusId='agony')],
        animations=anims('attack', 'attack', 'idle')))

    put_ability('bullet-ant/giant-killer', passive(
        'ability.ants.giant-killer', 'Giant Killer',
        'Bullet Ants go for the biggest prey: +15 damage against heroes and units of level 3 and up.',
        [{'bonus': 15}], [], icon('bullet-giant-killer'), [],
        combatModifiers={'attackBonus': {'amount': rp('bonus'), 'manaCost': 0, 'weapon': 'melee',
                                         'blockedBySilence': False, 'filter': {'minLevel': 3}}}))

    put_ability('herald/trail', ability(
        'ability.ants.trail', 'Trail',
        'Lay a pheromone trail for 12 seconds: allied units in it move 35% faster.',
        'targeted', [{}], point_aim(['ally'], 18, 5) | {'allowSelf': True}, cast(8, 6, 60, 15 * S),
        [{'op': 'status', 'target': 'target', 'id': 'trailed', 'amount': 30, 'polarity': 'positive', 'dispel': True,
          'modifiers': {'moveSpeedPermille': 350}}],
        icon('herald-trail'),
        [fx('windup', FX['cast-scent'], 'accepted', anchor='caster'),
         fx('lay', FX['trail-lay'], 'released'), fx('zone', FX['trail'], 'released'),
         fx('trailed', FX['trailed'], 'statusApplied', lifetime='status', statusId='trailed')],
        persistent={'durationTicks': 12 * S, 'intervalTicks': 20, 'anchor': 'point', 'endsWithCaster': False,
                    'upkeepMana': 0, 'toggle': False}))

    put_ability('herald/alarm', ability(
        'ability.ants.alarm', 'Alarm',
        'Sound the alarm: allied units within 9 units deal 35% more damage and attack 15% faster for 6 seconds.',
        'targeted', [{}], self_aim(), cast(8, 6, 120, 45 * S),
        [{'op': 'status', 'target': 'caster',
          'query': {'center': 'caster', 'radius': 9, 'relations': ['ally'], 'allowSelf': True, 'maxTargets': 32},
          'id': 'alarmed', 'amount': 6 * S, 'polarity': 'positive', 'dispel': True,
          'modifiers': {'damagePermille': 350, 'attackSpeedPermille': 150}}],
        icon('herald-alarm'),
        [fx('windup', FX['cast-war'], 'accepted', anchor='caster'),
         fx('alarm', FX['alarm'], 'released', anchor='caster'),
         fx('alarmed', FX['alarmed'], 'statusApplied', lifetime='status', statusId='alarmed')]))

    put_ability('herald/panic', ability(
        'ability.ants.panic', 'Panic',
        'A cloud of fear-scent: enemies in a 5-radius cannot attack or cast for 3 seconds (1.5 on heroes) and move 20% slower. Learned through Pheromone Mastery.',
        'targeted', [{}], point_aim(['enemy'], 16, 5), cast(10, 6, 100, 30 * S),
        [{'op': 'status', 'target': 'target', 'id': 'panicked', 'amount': 3 * S, 'heroDuration': 60,
          'polarity': 'negative', 'dispel': True, 'disarm': True, 'silence': True,
          'modifiers': {'moveSpeedPermille': -200}}],
        icon('herald-panic'),
        [fx('windup', FX['cast-scent'], 'accepted', anchor='caster'),
         fx('cloud', FX['panic'], 'released'),
         fx('panicked', FX['panicked'], 'statusApplied', lifetime='status', statusId='panicked')]))

    put_ability('gardener/fungus-circle', ability(
        'ability.ants.fungus-circle', 'Fungus Circle',
        'Grow a ring of glowing fungus for 10 seconds: allied units in it heal 15 health every second. The Reaver Queen is too far gone to be healed.',
        'targeted', [{}], point_aim(['ally'], 14, 5) | {'allowSelf': True}, cast(10, 6, 90, 20 * S),
        [{'op': 'heal', 'target': 'target', 'filter': {'maxLevel': 4}, 'amount': 15},
         {'op': 'heal', 'target': 'target', 'filter': {'heroes': True}, 'amount': 15}],
        icon('gardener-fungus-circle'),
        [fx('windup', FX['cast-scent'], 'accepted', anchor='caster'),
         fx('grow', FX['fungus-grow'], 'released'), fx('zone', FX['fungus'], 'released'),
         fx('heal', FX['fungus-heal'], 'healed')],
        persistent={'durationTicks': 10 * S, 'intervalTicks': S, 'anchor': 'point', 'endsWithCaster': False,
                    'upkeepMana': 0, 'toggle': False}))

    put_ability('gardener/spore-cloud', ability(
        'ability.ants.spore-cloud', 'Spore Cloud',
        'Release a cloud of choking spores for 8 seconds: enemies in it attack 40% slower and move 20% slower. Learned through Pheromone Mastery.',
        'targeted', [{}], point_aim(['enemy'], 14, 5), cast(10, 6, 100, 25 * S),
        [{'op': 'status', 'target': 'target', 'id': 'choked', 'amount': 30, 'polarity': 'negative', 'dispel': True,
          'modifiers': {'attackSpeedPermille': -400, 'moveSpeedPermille': -200}}],
        icon('gardener-spore-cloud'),
        [fx('windup', FX['cast-scent'], 'accepted', anchor='caster'),
         fx('zone', FX['spores'], 'released'),
         fx('choked', FX['choked'], 'statusApplied', lifetime='status', statusId='choked')],
        persistent={'durationTicks': 8 * S, 'intervalTicks': 20, 'anchor': 'point', 'endsWithCaster': False,
                    'upkeepMana': 0, 'toggle': False}))

    put_ability('reaver-queen/devour', ability(
        'ability.ants.devour', 'Devour',
        'Swallow a non-hero enemy whole and heal 300 health. It is digested over 10 seconds (35 damage a second); killing the Reaver Queen frees it.',
        'targeted', [{}], unit_aim(['enemy'], 3, filter={'heroes': False, 'buildings': False, 'locomotion': ['ground']}),
        cast(10, 10, 50, 20 * S),
        [{'op': 'contain', 'target': 'target', 'id': 'devoured', 'amount': 10 * S, 'lifetime': 'duration',
          'capacity': 1, 'digestion': {'intervalTicks': S, 'damage': 35, 'damageType': 'spell'}},
         {'op': 'heal', 'target': 'caster', 'amount': 300}],
        icon('reaver-devour'), [fx('devour', FX['devour'], 'contained'), fx('digest', FX['digest'], 'digested')],
        animations=anims('attack', 'attack', 'idle')))

    put_ability('reaver-queen/rampage', ability(
        'ability.ants.rampage', 'Rampage',
        'Charge an enemy: on contact, 90 damage and a 0.75-second stun to every enemy within 4 units.',
        'targeted', [{}], unit_aim(['enemy'], 20, filter={'locomotion': ['ground']}), cast(8, 8, 75, 18 * S),
        [{'op': 'damage', 'target': 'target',
          'query': {'center': 'target', 'radius': 4, 'relations': ['enemy'], 'maxTargets': 16},
          'amount': 90, 'damageType': 'melee'},
         {'op': 'status', 'target': 'target',
          'query': {'center': 'target', 'radius': 4, 'relations': ['enemy'], 'maxTargets': 16},
          'id': 'trampled', 'amount': 30, 'heroDuration': 15, 'polarity': 'negative', 'dispel': True, 'stun': True}],
        icon('reaver-rampage'), [fx('run', FX['rampage-run'], 'projectile', anchor='caster', durationFrom='delivery'),
                                   fx('hit', FX['rampage'], 'impact')],
        delivery={'kind': 'charge', 'speed': 34, 'maxTicks': 80},
        animations=anims('run', 'run', 'attack')))

    put_ability('reaver-queen/royal-presence', passive(
        'ability.ants.royal-presence', 'Royal Presence',
        'Ants within 10 units of the Reaver Queen attack 15% faster.',
        [{}],
        [{'op': 'status', 'target': 'target', 'id': 'presence', 'amount': 1, 'polarity': 'positive', 'dispel': False,
          'modifiers': {'attackSpeedPermille': 150}},
         {'op': 'status', 'target': 'caster', 'id': 'presenceSource', 'amount': 1, 'polarity': 'positive',
          'dispel': False}],
        icon('reaver-royal-presence'),
        [fx('presence', FX['presence'], 'statusApplied', lifetime='status', statusId='presence'),
         fx('source', FX['presence-source'], 'statusApplied', lifetime='status', statusId='presenceSource')],
        aura={'radius': 10, 'meleeOnly': False}))

    put_ability('reaver-queen/molt', ability(
        'ability.ants.molt', 'Molt',
        'Shed the old carapace: heal 35% of her health and move 30% faster for 10 seconds, with 3 less armor until the new one hardens.',
        'targeted', [{}], self_aim(), cast(16, 8, 100, 60 * S),
        [{'op': 'heal', 'target': 'caster', 'amount': 0,
          'scale': {'source': 'caster', 'stat': 'maxHealth', 'permille': 350}},
         {'op': 'status', 'target': 'caster', 'id': 'molting', 'amount': 10 * S, 'polarity': 'positive',
          'dispel': False, 'modifiers': {'moveSpeedPermille': 300, 'armor': -3}}],
        icon('reaver-molt'),
        [fx('windup', FX['cast-substance'], 'accepted', anchor='caster'),
         fx('molt', FX['molt'], 'healed'),
         fx('fresh', FX['molting'], 'statusApplied', lifetime='status', statusId='molting')]))

    put_ability('venom-spire/corrosion', passive(
        'ability.ants.corrosion', 'Corrosion',
        'Venom eats armor: each hit lowers the target\'s armor by 2 for 5 seconds.',
        [{'armor': -2}], [], icon('needler-barbed-needles'),
        [fx('corroded', FX['barbed'], 'statusApplied', lifetime='status', statusId='corroded')],
        combatModifiers={'attackBonus': {'status': 'corroded', 'amount': 0, 'manaCost': 0, 'weapon': 'projectile',
                                         'blockedBySilence': False}},
        statuses=[{'op': 'status', 'target': 'target', 'id': 'corroded', 'amount': 5 * S, 'polarity': 'negative',
                   'dispel': True, 'rankedModifiers': {'armor': rp('armor')}}]))

    put_ability('alate/found', ability(
        'ability.ants.found', 'Found',
        'The Alate lands, sheds its wings and becomes a Brood Mound where it stands: a new tunnel mound and 8 supply. The Alate is used up.',
        'targeted', [{}], self_aim(), cast(60, 0, 0, 0),
        [{'op': 'found', 'target': 'caster', 'building': MOUND, 'consume': True}],
        icon('found'), [fx('windup', FX['cast-royal'], 'accepted', anchor='caster'), fx('rise', FX['found'], 'summoned')],
        animations=anims('cast', 'cast', 'idle')))

    put_ability('reaver-queen/ravenous', passive(
        'ability.ants.ravenous', 'Ravenous',
        'Mutated. The Reaver Queen starves: she loses 10 health every second, and only Devour and Molt heal her.',
        [{}], [], icon('reaver-ravenous'), [],
        triggers=[{'id': 'starve', 'event': 'interval', 'intervalTicks': S, 'blockedBySilence': False,
                   'operations': [{'op': 'damage', 'target': 'caster', 'amount': 10, 'damageType': 'spell'}]}]))


# ----- hero abilities -----------------------------------------------------------------------

def hero_abilities():
    put_ability('heroes/queen/found-colony', ability(
        'ability.ants.found-colony', 'Found',
        'Raise a Brood Mound at a point nearby: a new tunnel mound and 8 supply. Cooldown 90/70/50 seconds.',
        'targeted', [{'cooldown': 90 * S}, {'cooldown': 70 * S}, {'cooldown': 50 * S}],
        point_aim(['ally'], 10, 1) | {'allowSelf': True}, cast(40, 8, 125, rp('cooldown')),
        [{'op': 'found', 'target': 'point', 'building': MOUND}],
        icon('found'), [fx('windup', FX['cast-royal'], 'accepted', anchor='caster'), fx('rise', FX['found'], 'summoned')]))

    put_ability('heroes/queen/royal-jelly', ability(
        'ability.ants.royal-jelly', 'Royal Jelly',
        'Feed an ally royal jelly: it heals 150/250/350 health and deals 20/30/40% more damage for 10 seconds.',
        'targeted', [{'heal': 150, 'boost': 200}, {'heal': 250, 'boost': 300}, {'heal': 350, 'boost': 400}],
        unit_aim(['ally'], 14, allow_self=True), cast(8, 6, 75, 12 * S),
        [{'op': 'heal', 'target': 'target', 'amount': rp('heal')},
         {'op': 'status', 'target': 'target', 'id': 'jellied', 'amount': 10 * S, 'polarity': 'positive',
          'dispel': True, 'rankedModifiers': {'damagePermille': rp('boost')}}],
        icon('queen-royal-jelly'),
        [fx('windup', FX['cast-royal'], 'accepted', anchor='caster'),
         fx('beam', FX['jelly-beam'], 'released', anchor='caster'),
         fx('jelly', FX['jelly'], 'statusApplied', statusId='jellied'),
         fx('jellied', FX['jellied'], 'statusApplied', lifetime='status', statusId='jellied')]))

    put_ability('heroes/queen/one-body', passive(
        'ability.ants.one-body', 'One Body',
        'The colony is one body: allies within 9 units of the Queen take 8/12/16% less damage.',
        [{'taken': 920}, {'taken': 880}, {'taken': 840}],
        [{'op': 'status', 'target': 'target', 'id': 'one-body', 'amount': 1, 'polarity': 'positive', 'dispel': False,
          'damageTakenPermille': {t: rp('taken') for t in PHYSICAL + ('spell',)}},
         {'op': 'status', 'target': 'caster', 'id': 'oneBodySource', 'amount': 1, 'polarity': 'positive',
          'dispel': False}],
        icon('queen-one-body'),
        [fx('one', FX['one-body'], 'statusApplied', lifetime='status', statusId='one-body'),
         fx('source', FX['one-body-source'], 'statusApplied', lifetime='status', statusId='oneBodySource')],
        aura={'radius': 9, 'meleeOnly': False}))

    put_ability('heroes/queen/exodus', ability(
        'ability.ants.exodus', 'Exodus',
        'Ultimate. Choose any friendly unit or building: after 2 seconds, the Queen and up to 32 allied units within 10 units of her sink underground and come up beside it.',
        'targeted', [{'cooldown': 120 * S}],
        unit_aim(['ally'], 4096, includeBuildings=True), cast(80, 8, 200, 120 * S),
        [{'op': 'teleport', 'target': 'target',
          'query': {'center': 'caster', 'radius': 10, 'relations': ['ally'], 'allowSelf': False,
                    'maxTargets': 32},
          'filter': {'buildings': False}, 'amount': 12, 'destination': 'target', 'preserveOffset': True},
         {'op': 'teleport', 'target': 'caster', 'amount': 12, 'destination': 'target', 'preserveOffset': False}],
        icon('queen-exodus'),
        [fx('sink', FX['exodus-gather'], 'accepted', anchor='caster'), fx('rise', FX['exodus-rise'], 'teleported')]))

    put_ability('heroes/royal-alchemist/volatile-draught', ability(
        'ability.ants.volatile-draught', 'Volatile Draught',
        'Throw a flask of substance venom: it bursts on the target for 90/140/190 spell damage to every enemy within 4 units.',
        'targeted', [{'damage': 90}, {'damage': 140}, {'damage': 190}],
        unit_aim(['enemy'], 16), cast(10, 8, 80, 8 * S),
        [{'op': 'damage', 'target': 'target',
          'query': {'center': 'target', 'radius': 4, 'relations': ['enemy'], 'maxTargets': 16},
          'amount': rp('damage'), 'damageType': 'spell'},
         {'op': 'damage', 'target': 'target', 'amount': rp('damage'), 'damageType': 'spell'}],
        icon('alchemist-volatile-draught'), [fx('windup', FX['cast-substance'], 'accepted', anchor='caster'),
                                              fx('flight', FX['draught-flight'], 'projectile', durationFrom='delivery'),
                                              fx('burst', FX['draught'], 'impact'), fx('burn', FX['draught-hit'], 'damaged')],
        delivery={'kind': 'projectile', 'speed': 26}))

    put_ability('heroes/royal-alchemist/transfusion', ability(
        'ability.ants.transfusion', 'Transfusion',
        'Draw 80/120/160 health out of an enemy and pour it into the most wounded ally within 12 units of the Alchemist.',
        'targeted', [{'drain': 80}, {'drain': 120}, {'drain': 160}],
        unit_aim(['enemy'], 12, filter={'buildings': False}), cast(10, 8, 70, 10 * S),
        [{'op': 'drain', 'target': 'target', 'resource': 'health', 'amount': rp('drain'), 'restoreCaster': False,
          'damageType': 'spell', 'damagePerDrainedPermille': 0, 'record': 'drawn'},
         {'op': 'heal', 'target': 'target',
          'query': {'center': 'caster', 'radius': 12, 'relations': ['ally'], 'allowSelf': True, 'maxTargets': 1,
                    'order': 'lowest-health'},
          'amount': 0, 'scale': {'source': 'result', 'id': 'drawn', 'stat': 'applied', 'permille': 1000}}],
        icon('alchemist-transfusion'),
        [fx('windup', FX['cast-substance'], 'accepted', anchor='caster'), fx('draw', FX['transfusion-draw'], 'drained', anchor='caster'),
         fx('pour', FX['transfusion-pour'], 'healed')]))

    put_ability('heroes/royal-alchemist/crystallise', ability(
        'ability.ants.crystallise', 'Crystallise',
        'Encase a unit in hardened substance for 2/3/4 seconds: an enemy is stunned (half as long on heroes); an ally cannot act but takes no damage.',
        'targeted', [{'hold': 2 * S, 'hero': S}, {'hold': 3 * S, 'hero': 60}, {'hold': 4 * S, 'hero': 2 * S}],
        unit_aim(['ally', 'enemy'], 14, allow_self=True, filter={'buildings': False}), cast(8, 6, 90, 16 * S),
        [{'op': 'branch', 'condition': {'kind': 'relation', 'of': 'target', 'to': 'caster', 'is': 'enemy'},
          'then': [{'op': 'status', 'target': 'target', 'id': 'crystal', 'amount': rp('hold'),
                    'heroDuration': rp('hero'), 'polarity': 'negative', 'dispel': True, 'stun': True}],
          'else': [{'op': 'status', 'target': 'target', 'id': 'sheltered', 'amount': rp('hold'),
                    'polarity': 'positive', 'dispel': True, 'stun': True, 'modifiers': {'invulnerable': True}}]}],
        icon('alchemist-crystallise'),
        [fx('windup', FX['cast-substance'], 'accepted', anchor='caster'), fx('form', FX['crystal-form'], 'statusApplied'),
         fx('crystal', FX['crystal'], 'statusApplied', lifetime='status', statusId='crystal'),
         fx('shelter', FX['crystal'], 'statusApplied', lifetime='status', statusId='sheltered')]))

    put_ability('heroes/royal-alchemist/overdose', ability(
        'ability.ants.overdose', 'Overdose',
        'Ultimate. Allies in a 6-radius swell with substance for 10 seconds: they attack 40/60% faster, deal 25/40% more damage and move 20% faster, but lose 10 health every second while it lasts.',
        'targeted', [{'haste': 400, 'power': 250}, {'haste': 600, 'power': 400}],
        point_aim(['ally'], 18, 6, maxTargets=32) | {'allowSelf': True}, cast(12, 8, 175, 120 * S),
        [{'op': 'status', 'target': 'target', 'id': 'overdosed', 'amount': 10 * S, 'polarity': 'positive',
          'dispel': False, 'modifiers': {'moveSpeedPermille': 200},
          'rankedModifiers': {'attackSpeedPermille': rp('haste'), 'damagePermille': rp('power')},
          'periodic': {'intervalTicks': S, 'damage': 10, 'damageType': 'spell'}}],
        icon('alchemist-overdose'),
        [fx('windup', FX['cast-substance'], 'accepted', anchor='caster'),
         fx('pour', FX['overdose'], 'released'),
         fx('swell', FX['overdosed'], 'statusApplied', lifetime='status', statusId='overdosed')]))

    put_ability('heroes/general/mandible-charge', ability(
        'ability.ants.mandible-charge', 'Mandible Charge',
        'Rush an enemy and seize it in the mandibles: 80/130/180 damage and a 1/1.5/2-second stun.',
        'targeted', [{'damage': 80, 'stun': S}, {'damage': 130, 'stun': 60}, {'damage': 180, 'stun': 2 * S}],
        unit_aim(['enemy'], 18, filter={'locomotion': ['ground'], 'buildings': False}), cast(6, 8, 75, 12 * S),
        [{'op': 'damage', 'target': 'target', 'amount': rp('damage'), 'damageType': 'hero'},
         {'op': 'status', 'target': 'target', 'id': 'seized', 'amount': rp('stun'), 'polarity': 'negative',
          'dispel': True, 'stun': True}],
        icon('general-mandible-charge'), [fx('run', FX['charge-trail'], 'projectile', anchor='caster', durationFrom='delivery'),
                                           fx('hit', FX['charge-hit'], 'impact')],
        delivery={'kind': 'charge', 'speed': 36, 'maxTicks': 80},
        animations=anims('run', 'run', 'attack')))

    put_ability('heroes/general/mark-prey', ability(
        'ability.ants.mark-prey', 'Mark Prey',
        'Mark an enemy with hunting scent for 12 seconds: it takes 15/25/35% more damage from everything.',
        'targeted', [{'taken': 1150}, {'taken': 1250}, {'taken': 1350}],
        unit_aim(['enemy'], 18), cast(6, 6, 50, 10 * S),
        [{'op': 'status', 'target': 'target', 'id': 'marked', 'amount': 12 * S, 'polarity': 'negative',
          'dispel': True, 'damageTakenPermille': {t: rp('taken') for t in PHYSICAL + ('spell',)}}],
        icon('general-mark-prey'),
        [fx('windup', FX['cast-war'], 'accepted', anchor='caster'), fx('hit', FX['mark-hit'], 'statusApplied'),
         fx('mark', FX['marked'], 'statusApplied', lifetime='status', statusId='marked')]))

    put_ability('heroes/general/war-column', passive(
        'ability.ants.war-column', 'War Column',
        'Allies within 10 units of the General move 10/15/20% faster and deal 10/15/20% more damage.',
        [{'boost': 100}, {'boost': 150}, {'boost': 200}],
        [{'op': 'status', 'target': 'target', 'id': 'column', 'amount': 1, 'polarity': 'positive', 'dispel': False,
          'rankedModifiers': {'moveSpeedPermille': rp('boost'), 'damagePermille': rp('boost')}},
         {'op': 'status', 'target': 'caster', 'id': 'columnSource', 'amount': 1, 'polarity': 'positive',
          'dispel': False}],
        icon('general-war-column'),
        [fx('column', FX['column'], 'statusApplied', lifetime='status', statusId='column'),
         fx('source', FX['column-source'], 'statusApplied', lifetime='status', statusId='columnSource')],
        aura={'radius': 10, 'meleeOnly': False}))

    put_ability('heroes/general/bivouac', ability(
        'ability.ants.bivouac', 'Bivouac',
        'Ultimate. The General and up to 24 allies within 7 units lock together into a living fortress for 8/10 seconds: they cannot move, take 60/70% less damage and return 35% of melee damage to attackers.',
        'targeted', [{'hold': 8 * S, 'taken': 400}, {'hold': 10 * S, 'taken': 300}],
        self_aim(), cast(10, 6, 150, 90 * S),
        [{'op': 'status', 'target': 'caster',
          'query': {'center': 'caster', 'radius': 7, 'relations': ['ally'], 'allowSelf': True, 'maxTargets': 25},
          'filter': {'buildings': False}, 'id': 'bivouac', 'amount': rp('hold'), 'polarity': 'positive',
          'dispel': False, 'modifiers': {'rooted': True, 'meleeReflectionPermille': 350},
          'damageTakenPermille': {t: rp('taken') for t in PHYSICAL + ('spell',)}}],
        icon('general-bivouac'),
        [fx('windup', FX['cast-war'], 'accepted', anchor='caster'),
         fx('lock', FX['bivouac'], 'released', anchor='caster'),
         fx('fort', FX['bivouacked'], 'statusApplied', lifetime='status', statusId='bivouac')]))


# ----- research -----------------------------------------------------------------------------

def write_research():
    ids = {}

    def put(name, title, description, icon_, priority, items, seconds, effects, requires=None, follows=None):
        # A level painted on its own (D79: each level of a line shows the object improving)
        # keeps its own icon.
        if os.path.isdir(os.path.join(BASE, 'assets/icons/research/ants', name)):
            icon_ = 'asset.icons.research-ants-' + name
        d = {'id': r(name), 'name': title, 'description': description, 'icon': icon_, 'priority': priority}
        if requires:
            d['requires'] = requires
        if follows:
            d['follows'] = follows
        d['items'] = items
        d['workTicks'] = int(seconds * S)
        d['effects'] = effects
        write('research/ants/' + name + '.json', d)
        ids[name] = d['id']
        return d['id']

    melee = [RAIDER, MAJOR, BULLET]
    ranged = [NEEDLER, HONEYPOT, ALATE, SPITTER]
    armoured = [RAIDER, NEEDLER, BURSTER, MAJOR, HONEYPOT, ALATE, BULLET, HERALD, GARDENER, REAVER]
    tiers = [(None, 'I', 125, 0, 40), (T2, 'II', 175, 50, 50), (T3, 'III', 225, 100, 60)]
    works = []
    for key, label, what, effect_, prio in (
            ('mandibles', 'Mandibles', 'Raiders, Majors and Bullet Ants deal 10% more attack damage',
             {'units': melee, 'damagePermille': 100}, 90),
            ('needle-hardening', 'Needle Hardening',
             'Needlers, Honeypots, Alates and Acid Spires deal 10% more attack damage',
             {'units': ranged, 'damagePermille': 100}, 85),
            ('carapace', 'Carapace', 'Ant troops gain 1 armor', {'units': armoured, 'armor': 1}, 80)):
        prev = None
        for i, (hall, roman, amber, substance, seconds) in enumerate(tiers):
            gate = {None: '', T2: ' Requires a Great Nest.', T3: " Requires the Queen's Chamber."}[hall]
            rid = put(f'{key}-{i + 1}', f'{label} {roman}', f'{what} (level {roman}; levels add up).{gate}',
                      ricon(key), prio - i, cost(amber, substance), seconds, [effect_],
                      requires=[x for x in (hall, prev) if x] or None, follows=prev)
            works.append(rid)
            prev = rid
    ids['works'] = works
    garden = []
    for name, title, description, key, prio, price, seconds, effects, requires in (
            ('barbed-needles', 'Barbed Needles', 'Needler hits lower the target\'s armor by 1 for 5 seconds, up to 3.',
             'barbed-needles', 80, cost(100, 50), 40, [{'units': [NEEDLER], 'abilityRanks': {'barbed-needles': 1}}], None),
            ('volatile-sacs', 'Volatile Sacs', 'Burster bursts deal 150 damage instead of 100 and no longer hurt allies.',
             'volatile-sacs', 75, cost(100, 75), 40, [{'units': [BURSTER], 'abilityRanks': {'unstable': 2}}], None),
            ('living-wall', 'Living Wall', 'Majors gain 2 armor, and their Guard cuts the damage small ants take by 25% instead of 15%.',
             'living-wall', 70, cost(150, 75), 45, [{'units': [MAJOR], 'armor': 2, 'abilityRanks': {'guard': 2}}], None),
            ('sticky-nectar', 'Sticky Nectar', 'Honeypot globs splash wider (3.4 units) and leave the survivors 30% slower for a while.',
             'sticky-nectar', 70, cost(125, 75), 45, [{'units': [HONEYPOT], 'splashRadius': 3.4, 'splashSlowPermille': 300}], None),
            ('pheromone-mastery', 'Pheromone Mastery', 'Heralds learn Panic and Gardeners learn Spore Cloud.',
             'pheromone-mastery', 85, cost(150, 100), 50,
             [{'units': [HERALD], 'abilityRanks': {'panic': 1}}, {'units': [GARDENER], 'abilityRanks': {'spore-cloud': 1}}],
             [SCENT_HALL]),
            ('hardened-wings', 'Hardened Wings', 'Alates gain 100 health and fly 20% faster.',
             'hardened-wings', 60, cost(125, 75), 40, [{'units': [ALATE], 'maxHp': 100, 'moveSpeedPermille': 200}], None),
            ('venom-glands', 'Venom Glands', 'Agony lingers for 10 seconds instead of 6. Requires the Queen\'s Chamber.',
             'venom-glands', 65, cost(150, 100), 50, [{'units': [BULLET], 'abilityRanks': {'agony': 2}}], [T3])):
        garden.append(put(name, title, description, ricon(key), prio, price, seconds, effects, requires=requires))
    ids['garden'] = garden
    tunnels = []
    if ENGINE:
        prev = None
        for i, (hall, roman, amber, substance, seconds) in enumerate(((None, 'I', 100, 50, 40), (T3, 'II', 175, 100, 50))):
            gate = " Requires the Queen's Chamber." if hall else ''
            rid = put(f'wider-tunnels-{i + 1}', f'Wider Tunnels {roman}',
                      f'Every mound lets 50% more ants into the tunnels each second (level {roman}; levels add up).{gate}',
                      ricon('wider-tunnels'), 50 - i, cost(amber, substance), seconds,
                      [{'units': [NEST, GREAT_NEST, CHAMBER, MOUND], 'tunnelThroughputPermille': 500}],
                      requires=[x for x in (hall, prev) if x] or None, follows=prev)
            tunnels.append(rid)
            prev = rid
    ids['tunnels'] = tunnels
    return ids


# ----- units --------------------------------------------------------------------------------

def load(rel):
    return json.load(open(BASE + rel))


def weapon(damage, kind, range_, cooldown, aggro, windup, recovery, buffer=0.6, **extra):
    c = {'damage': damage, 'damageType': kind, 'range': range_, 'cooldownTicks': cooldown, 'aggroRange': aggro,
         'attack': {'windupTicks': windup, 'recoveryTicks': recovery, 'rangeBuffer': buffer}}
    c.update(extra)
    return c


def train(amber, substance, seconds, **extra):
    return {'method': 'train', 'items': cost(amber, substance), 'workTicks': int(seconds * S)} | extra


def body(hp, armor, kind):
    return {'maxHp': hp, 'armor': armor, 'armorType': kind}


def caster(mana, regen, bindings):
    return {'maxMana': mana, 'manaRegenPerSecond': regen, 'bindings': bindings}


def hero_levels(hp, hp_step, dmg, dmg_step, armor, mana, mana_step, cooldown, cd_floor, hregen, mregen):
    xp = [0, 160, 400, 720, 1120, 1600, 2160, 2800, 3520, 4320, 5200]
    return [{'experience': e, 'maxHp': hp + hp_step * i, 'damage': dmg + round(dmg_step * i),
             'armor': armor + i // 2, 'maxMana': mana + mana_step * i,
             'cooldownTicks': max(cd_floor, cooldown - 2 * i),
             'healthRegenPerSecond': round(hregen + 0.15 * i, 2),
             'manaRegenPerSecond': round(mregen + 0.08 * i, 2)} for i, e in enumerate(xp)]


ORDER = ['id', 'name', 'description', 'model', 'icon', 'hero', 'selectionClass', 'body', 'vision', 'behaviorSets',
         'behaviors', 'creation', 'requires', 'level', 'supplyCost', 'maxOwned', 'unitNature', 'dimensions',
         'modelScale', 'presentation', 'selectable', 'displayOrder']


def ordered(d):
    return {k: d[k] for k in ORDER if k in d} | {k: v for k, v in d.items() if k not in ORDER}


def unit(rel, *, name, description, body_, combat, speed, creation, supply, level, requires=None, abilities=None,
         movement=None, max_owned=None, presentation=None, base=None, extra_behaviors=None, underground=None,
         scale=None):
    """Patches the gameplay of an existing definition (its art and presentation stay)."""
    d = load(base or rel)
    d['name'] = name
    d['description'] = description
    d['body'] = body_
    beh = {k: v for k, v in d['behaviors'].items() if k not in ('combat', 'movement', 'abilities')}
    if combat:
        beh['combat'] = combat
    beh['movement'] = {'speed': speed} | (movement or {})
    if abilities:
        beh['abilities'] = abilities
    beh.update(extra_behaviors or {})
    d['behaviors'] = beh
    d['creation'] = creation
    d.pop('requires', None)
    if requires:
        d['requires'] = requires
    d['level'] = level
    d['supplyCost'] = supply
    d.pop('maxOwned', None)
    if max_owned:
        d['maxOwned'] = max_owned
    if presentation is not None:
        d['presentation'] = presentation
    if scale:
        d['modelScale'] = scale
    d.pop('underground', None)
    if underground is not None and ENGINE:
        d['underground'] = underground
    write(rel, ordered(d))
    return d


def passive_binding(id_, ability_id, rank=1):
    return binding(id_, ability_id, rank, None, None)


def write_units(builds):
    unit('units/ants/settler.json', name='Worker',
         description='Gathers amber, builds the colony and, like every ant, travels through the tunnels. Uses 1 supply.',
         body_=body(240, 0, 'light'), combat=None, speed=12, creation=train(100, 0, 15), supply=1, level=1,
         extra_behaviors={'work': load('units/ants/settler.json')['behaviors']['work'] | {'builds': builds}})
    unit('units/ants/warrior.json', name='Raider',
         description='The colony\'s Zergling: a small, stocky biting ant with blade fins on its back, hatched two at a time. Fast and cheap; Frenzy makes Raiders hit harder the longer they fight together. Uses 1 supply each.',
         body_=body(185, 1, 'heavy'), combat=weapon(8, 'melee', 0.6, 36, 20, 8, 8), speed=15,
         creation=train(150, 0, 24, **({'count': 2} if ENGINE else {})), supply=1, level=1,
         abilities=caster(10, 0, [passive_binding('frenzy', 'ability.ants.frenzy')]))
    archer = load('units/ants/archer.json')
    unit('units/ants/archer.json', name='Needler',
         description='An ant creature bristling with chitin needles that it fires at ground and air targets. Barbed Needles, from the Fungus Garden, makes each hit strip armor. Uses 2 supply.',
         body_=body(380, 0, 'light'),
         combat=weapon(15, 'piercing', 18, 44, 22, 10, 10, 0.75, projectile={'speed': 40}, targets=['ground', 'air']),
         speed=11, creation=train(150, 0, 22), supply=2, level=1,
         abilities=caster(10, 0, [passive_binding('barbed-needles', 'ability.ants.barbed-needles', 0)]),
         presentation=archer.get('presentation', {}) | {'projectile': 'effect.weapon.thorn'})
    unit('units/ants/burster.json', name='Burster',
         description='Mutated: a huge sloshing sac of liquid substance on legs. It bursts on its first bite, or on command, for heavy damage to enemies and buildings around it, and hurts nearby allies too. Too bloated for the tunnels. Requires a Great Nest; uses 1 supply.',
         body_=body(220, 0, 'light'), combat=weapon(10, 'melee', 0.6, 40, 20, 6, 6), speed=14,
         creation=train(110, 60, 20), supply=1, level=2, requires=[T2], underground=False,
         abilities=caster(10, 0, [passive_binding('unstable', 'ability.ants.unstable'),
                                  binding('rupture', 'ability.ants.rupture', 1, 'Q', 1, controls=('player', 'scenario'))]),
         presentation={})
    unit('units/ants/queensguard.json', name='Major',
         description='The colony\'s armoured tank, with a huge armoured head and a great shield. Guard: small ants beside it take less damage. Plug: it plants its head like a door and holds. Requires a Great Nest; uses 3 supply.',
         body_=body(960, 4, 'heavy'), combat=weapon(24, 'melee', 0.8, 64, 20, 14, 12), speed=9,
         creation=train(230, 60, 28), supply=3, level=2, requires=[T2], scale=4.2,
         abilities=caster(10, 0, [passive_binding('guard', 'ability.ants.guard'),
                                  binding('plug', 'ability.ants.plug', 1, 'Q', 1, threat=7)]))
    bomb = load('units/ants/bombardier.json')
    unit('units/ants/bombardier.json', name='Honeypot',
         description='Mutated: a swollen amber replete fused with its carriers. Lobs bursting nectar globs over long range, but cannot fire at anything within 7 units, is slow, and too bloated for the tunnels. Requires a Great Nest; uses 3 supply.',
         body_=body(540, 1, 'light'),
         combat=weapon(60, 'siege', 30, 140, 32, 24, 20, 0.75, minRange=7.0,
                       shell={'flightTicks': 46, 'radius': 2.6, 'slowPermille': 0, 'slowTicks': 0}),
         speed=7, creation=train(240, 60, 30), supply=3, level=2, requires=[T2], underground=False,
         presentation=bomb.get('presentation', {}) | {'projectile': 'effect.weapon.nectar'})
    unit('units/ants/winglancer.json', name='Alate',
         description='A fast winged ant: scout and light harasser that strikes ground targets from the air. Found: it lands, sheds its wings and becomes a new tunnel mound. Requires a Great Nest; uses 2 supply.',
         body_=body(360, 1, 'light'), combat=weapon(18, 'melee', 1.5, 44, 22, 10, 10), speed=18,
         movement={'turnRate': 540, 'locomotion': 'air', 'flightHeight': 6},
         creation=train(175, 50, 24), supply=2, level=2, requires=[T2],
         abilities=caster(100, 0.5, [binding('found', 'ability.ants.found', 1, 'Q', 1, interval=80)]) if ENGINE else None)
    unit('units/ants/thornblade.json', name='Bullet Ant',
         description='Elite duellist with a bristling mane, hunting the biggest prey: +15 damage against heroes and large units. Agony: a sting that stuns, then slows and weakens. Requires the Queen\'s Chamber; uses 3 supply.',
         body_=body(560, 2, 'heavy'), combat=weapon(22, 'melee', 0.6, 48, 22, 10, 10), speed=13,
         creation=train(200, 75, 28), supply=3, level=3, requires=[T3],
         abilities=caster(150, 0.8, [binding('agony', 'ability.ants.agony', 1, 'Q', 1, interval=12),
                                     passive_binding('giant-killer', 'ability.ants.giant-killer')]))
    herald = load('units/ants/banneret.json')
    unit('units/ants/banneret.json', name='Herald',
         description='Pheromone caster with a crown of golden feathers. Trail speeds allies along the ground, Alarm drives them into a fury, and Panic, learned through Pheromone Mastery, leaves enemies unable to fight. Requires a Great Nest; uses 2 supply.',
         body_=body(420, 1, 'light'),
         combat=weapon(9, 'spell', 12, 60, 18, 14, 10, 0.75, projectile={'speed': 30}), speed=11,
         creation=train(160, 40, 24), supply=2, level=2, requires=[T2],
         abilities=caster(250, 1.1, [binding('trail', 'ability.ants.trail', 1, 'Q', 1, preference='wounded-ally', interval=40, threat=20),
                                     binding('alarm', 'ability.ants.alarm', 1, 'W', 2, threat=10),
                                     binding('panic', 'ability.ants.panic', 0, 'E', 3)]),
         presentation=herald.get('presentation', {}) | {'projectile': 'effect.weapon.nectar', 'projectileSocket': 'socket_projectile'})
    garden = load('units/ants/nectarist.json')
    unit('units/ants/nectarist.json', name='Gardener',
         description='Fungus keeper under a wide mushroom cap. Fungus Circle heals the allies standing in it; Spore Cloud, learned through Pheromone Mastery, chokes enemies. Requires a Great Nest; uses 2 supply.',
         body_=body(400, 0, 'light'),
         combat=weapon(8, 'spell', 13, 64, 18, 14, 10, 0.75, projectile={'speed': 30}), speed=11,
         creation=train(150, 40, 24), supply=2, level=2, requires=[T2],
         abilities=caster(240, 1.2, [binding('fungus-circle', 'ability.ants.fungus-circle', 1, 'Q', 1, preference='wounded-ally', interval=30),
                                     binding('spore-cloud', 'ability.ants.spore-cloud', 0, 'W', 2)]),
         presentation=garden.get('presentation', {}))
    unit('units/ants/reaver-queen.json', name='Reaver Queen',
         description='Mutated: the colony\'s ultimate monster, one at a time. Devours enemies whole, Rampages through their lines, and Molts to heal; nearby ants attack faster in her presence. Ravenous: she starves without prey, Gardeners cannot heal her, and she is too big for the tunnels. Requires the Queen\'s Chamber; uses 8 supply.',
         body_=body(2800, 5, 'heavy'), combat=weapon(55, 'melee', 1.5, 60, 24, 16, 14), speed=9,
         creation=train(550, 250, 60), supply=8, level=5, requires=[T3], max_owned=1, underground=False,
         abilities=caster(200, 1.0, [binding('devour', 'ability.ants.devour', 1, 'Q', 1, interval=20),
                                     binding('rampage', 'ability.ants.rampage', 1, 'W', 2, interval=40),
                                     binding('molt', 'ability.ants.molt', 1, 'E', 3, preference='wounded-ally', interval=40),
                                     passive_binding('royal-presence', 'ability.ants.royal-presence'),
                                     passive_binding('ravenous', 'ability.ants.ravenous')]),
         presentation={'cameraAnchor': {'height': 1.4, 'forward': 0.2, 'distance': 9}})
    # The Acid Spire's crew: a Needler in the spire's mouth, spitting at ground and air.
    for name, title, dmg, kind, rank in (('spire-spitter', 'Spire Spitter', 20, 'piercing', 0),
                                         ('venom-spitter', 'Venom Spitter', 26, 'spell', 1)):
        d = {'id': u(name), 'name': title, 'description': 'The spitter in an ant spire\'s mouth.',
             'model': 'asset.models.units.ants-needler', 'icon': 'asset.portraits.units.ants-needler',
             'body': body(100, 0, 'light'), 'vision': 30, 'behaviorSets': [],
             'behaviors': {'combat': weapon(dmg, kind, 20, 44, 22, 8, 8, 0.75, projectile={'speed': 36},
                                            targets=['ground', 'air'])},
             'level': 1, 'supplyCost': 0, 'dimensions': {'radius': 0.8, 'height': 2.0, 'formationSpacing': 2.0},
             'modelScale': 1.6, 'presentation': {'projectile': 'effect.weapon.thorn'}, 'selectable': False}
        if rank:
            d['behaviors']['abilities'] = caster(10, 0, [passive_binding('corrosion', 'ability.ants.corrosion')])
        write('units/ants/' + name + '.json', d)

    def hero(rel, base, name, description, levels, combat, speed, regen, bindings, order, presentation=None):
        d = unit(rel, base=base, name=name, description=description,
                 body_=body(levels[0]['maxHp'], levels[0]['armor'], 'hero'), combat=combat, speed=speed,
                 creation=train(425, 75, 55), supply=4, level=1, scale=6.0,
                 abilities=caster(levels[0]['maxMana'], regen, bindings), presentation=presentation)
        d['behaviors']['progression'] = d['behaviors'].get('progression', {}) | {'experienceRadius': 40, 'levels': levels}
        d['hero'] = True
        d['displayOrder'] = order
        write(rel, ordered(d))

    ult = [6, 9]
    hero('units/ants/heroes/queen.json', None, 'The Queen',
         'The colony\'s mother and the campaign\'s protagonist. Founds new mounds where she stands, feeds allies Royal Jelly, shares the colony\'s wounds through One Body and, at the last, leads the Exodus underground.',
         hero_levels(720, 75, 24, 3.0, 2, 260, 26, 72, 60, 1.3, 1.0),
         weapon(24, 'hero', 14, 72, 20, 18, 12, 0.75, projectile={'speed': 36}), 11, 1.0,
         ([binding('found', 'ability.ants.found-colony', 0, 'Q', 1, levels=[1, 3, 5])] if ENGINE else []) + [
          binding('royal-jelly', 'ability.ants.royal-jelly', 0, 'W', 2, levels=[1, 3, 5], preference='wounded-ally'),
          binding('one-body', 'ability.ants.one-body', 0, 'E', 3, levels=[1, 3, 5]),
          binding('exodus', 'ability.ants.exodus', 0, 'R', 4, levels=[6], interval=40)
          | {'ai': {'preference': 'enemy', 'intervalTicks': 40, 'intent': 'reinforce'}}], 1,
         presentation={'projectile': 'effect.weapon.nectar', 'projectileSocket': 'socket_projectile', 'castContact': 0.5})
    hero('units/ants/heroes/royal-alchemist.json', None, 'Royal Alchemist',
         'Distils substance in her own body. Throws Volatile Draughts, moves life from foe to friend with Transfusion, Crystallises friend or foe and, at the last, drives the army into an Overdose.',
         hero_levels(660, 70, 22, 3.0, 1, 300, 30, 72, 60, 1.2, 1.1),
         weapon(22, 'hero', 14, 72, 20, 18, 12, 0.75, projectile={'speed': 36}), 11, 1.1,
         [binding('volatile-draught', 'ability.ants.volatile-draught', 0, 'Q', 1, levels=[1, 3, 5]),
          binding('transfusion', 'ability.ants.transfusion', 0, 'W', 2, levels=[1, 3, 5]),
          binding('crystallise', 'ability.ants.crystallise', 0, 'E', 3, levels=[1, 3, 5]),
          binding('overdose', 'ability.ants.overdose', 0, 'R', 4, levels=ult, threat=14)], 2,
         presentation={'projectile': 'effect.weapon.nectar', 'projectileSocket': 'socket_projectile', 'castContact': 0.5})
    hero('units/ants/heroes/marshal.json', None, 'General',
         'The army-ant marshal, banner on its back and twin blades in hand. Mandible Charge seizes a foe, Mark Prey sets the army on it, War Column drives the march and, at the last, the army locks into a Bivouac.',
         hero_levels(900, 90, 44, 4.5, 4, 200, 20, 78, 64, 1.8, 0.75),
         weapon(44, 'hero', 0.8, 78, 20, 18, 14), 12, 0.75,
         [binding('mandible-charge', 'ability.ants.mandible-charge', 0, 'Q', 1, levels=[1, 3, 5]),
          binding('mark-prey', 'ability.ants.mark-prey', 0, 'W', 2, levels=[1, 3, 5]),
          binding('war-column', 'ability.ants.war-column', 0, 'E', 3, levels=[1, 3, 5]),
          binding('bivouac', 'ability.ants.bivouac', 0, 'R', 4, levels=ult, threat=9)], 3)


# ----- buildings ----------------------------------------------------------------------------

def building(rel, *, name, description, behaviors=None, upgrade=None, requires=None, extra=None, drop=()):
    d = load(rel)
    d['name'] = name
    d['description'] = description
    if behaviors is not None:
        d['behaviors'] = behaviors
    d.pop('upgrade', None)
    if upgrade:
        d['upgrade'] = upgrade
    if requires is not None:
        d.pop('requires', None)
        if requires:
            d['requires'] = requires
    for k in drop:
        d.pop(k, None)
    d.update(extra or {})
    write(rel, d)
    return d


def bills(capacity):
    return {'storage': {'capacity': capacity, 'accepts': ['item.amber', 'item.root']}}


def queued(outputs, capacity=6):
    return {'mode': 'queued', 'outputs': outputs, 'workerSlots': 0, 'queueCapacity': capacity}


TUNNEL = {'intervalTicks': 12, 'speed': 60}


def write_buildings(research):
    hall = load('buildings/ants/fort.json')['behaviors']
    mound = {'tunnel': TUNNEL} if ENGINE else {}
    building('buildings/ants/fort.json', name='Nest',
             description='The colony\'s main hall and its first tunnel mound: stores goods, hatches Workers and provides 15 supply. Ants travel between mounds underground. Upgrades to the Great Nest.',
             behaviors=hall | {'production': queued([WORKER])},
             upgrade={'target': T2, 'items': cost(400, 150), 'workTicks': 80 * S}, extra=mound)
    building('buildings/ants/great-mound.json', name='Great Nest',
             description='Hall tier 2 and a tunnel mound: unlocks the Fungus Garden, the Scent Hall, the Deep Gallery, tier-2 troops and level II research. Upgrades to the Queen\'s Chamber once a Deep Gallery stands.',
             behaviors=hall | {'production': queued([WORKER])},
             upgrade={'target': T3, 'items': cost(600, 250), 'workTicks': 100 * S}, extra=mound)
    building('buildings/ants/elder-hall.json', name='Queen\'s Chamber',
             description='Hall tier 3 and a tunnel mound: unlocks the Queen\'s Cradle, the Bullet Ant, the Venom Spire, level III research and Wider Tunnels II.',
             behaviors=hall | {'production': queued([WORKER])}, requires=[GALLERY], extra=mound)
    building('buildings/ants/house.json', name='Brood Mound',
             description='Brood chambers around a tunnel shaft: provides 8 supply and is a tunnel mound. Researches Wider Tunnels.',
             behaviors={'research': {'outputs': research['tunnels'], 'queueCapacity': 2}} if research['tunnels'] else {},
             extra=mound)
    building('buildings/ants/barracks.json', name='Soldier Burrow',
             description='Hatches Raiders (two at a time), Needlers, Bursters and Majors.',
             behaviors={k: v for k, v in load('buildings/ants/barracks.json')['behaviors'].items() if k != 'research'}
             | bills(800) | {'production': queued([RAIDER, NEEDLER, BURSTER, MAJOR])})
    building('buildings/ants/ironroot-forge.json', name='Chitin Works',
             description='Grows weapons and armour for the colony: Mandibles, Needle Hardening and Carapace, three levels each. Level II needs a Great Nest, level III the Queen\'s Chamber.',
             behaviors={'research': {'outputs': research['works'], 'queueCapacity': 3}})
    building('buildings/ants/fungus-garden.json', name='Fungus Garden',
             description='The colony\'s substance garden: one-off upgrades for every caste. Requires a Great Nest.',
             behaviors={'research': {'outputs': research['garden'], 'queueCapacity': 3}}, requires=[T2])
    building('buildings/ants/scent-hall.json', name='Scent Hall',
             description='Where the pheromone castes are raised: hatches Heralds and Gardeners. Requires a Great Nest.',
             behaviors=bills(800) | {'production': queued([HERALD, GARDENER])}, requires=[T2])
    building('buildings/ants/bombardier-workshop.json', name='Deep Gallery',
             description='Galleries of the specialised castes: hatches Honeypots and Alates, and Bullet Ants once the Queen\'s Chamber stands. Requires a Great Nest.',
             behaviors=bills(1200) | {'production': queued([HONEYPOT, ALATE, BULLET])}, requires=[T2])
    building('buildings/ants/queens-cradle.json', name='Queen\'s Cradle',
             description='The amber reliquary where a Reaver Queen is raised, one at a time. Requires the Queen\'s Chamber.',
             behaviors=bills(1200) | {'production': queued([REAVER], 1)}, requires=[T3])
    crew = lambda unit_: {'capacity': 1, 'accepts': [], 'height': 5.8, 'lookoutRadius': 1.35, 'crew': unit_}
    spire = building('buildings/ants/tower.json', name='Acid Spire',
                     description='A spire with a Needler spitting from its mouth at ground and air units. Upgrades to the Venom Spire once the Queen\'s Chamber stands.',
                     upgrade={'target': VENOM_SPIRE, 'items': cost(125, 50), 'workTicks': 40 * S},
                     extra={'garrison': crew(SPITTER)})
    venom = copy.deepcopy(spire)
    venom.update({'id': VENOM_SPIRE, 'name': 'Venom Spire',
                  'description': 'An Acid Spire grown venomous: its spitter deals 26 spell damage and each hit lowers armor by 2. Requires the Queen\'s Chamber.',
                  'icon': 'asset.portraits.buildings.ants-venom-spire', 'requires': [T3], 'level': 3,
                  'body': spire['body'] | {'maxHp': spire['body']['maxHp'] + 250, 'armor': spire['body']['armor'] + 1},
                  'garrison': crew(u('venom-spitter'))})
    for k in ('upgrade', 'creation', 'command', 'category'):
        venom.pop(k, None)
    write('buildings/ants/venom-spire.json', venom)
    royal = load('buildings/ants/sanctuary.json')['behaviors']
    building('buildings/ants/sanctuary.json', name='Royal Chamber',
             description='Where the colony\'s heroes are raised and revived: the Queen, the Royal Alchemist and the General.',
             behaviors=royal | bills(100000) | {'production': royal['production'] | {'outputs': [QUEEN, ALCHEMIST, GENERAL]}})


def weapon_effects():
    """Every ant blow lands with weight: an impact (and a spit at the mouth) per weapon."""
    hits = {'warrior': ('hit-bite', None), 'archer': ('hit-needle', 'effect.weapons.thorn-flight'),
            'queensguard': ('hit-heavy', None),
            'bombardier': ('glob-impact', 'glob-flight'), 'winglancer': ('hit-needle', None),
            'thornblade': ('hit-sting', None),
            'banneret': ('nectar-hit', 'nectar-flight'), 'nectarist': ('nectar-hit', 'nectar-flight'),
            'reaver-queen': ('hit-reaver', None),
            'heroes/marshal': ('hit-heavy', None), 'heroes/queen': ('nectar-hit', 'royal-nectar-flight'),
            'heroes/royal-alchemist': ('nectar-hit', 'royal-nectar-flight'),
            'spire-spitter': ('hit-needle', 'spit-thorn'), 'venom-spitter': ('hit-sting', 'spit-venom')}
    for name, (impact, launch) in hits.items():
        rel = 'units/ants/' + name + '.json'
        d = load(rel)
        pres = d.get('presentation', {}) | {'impactEffect': FX[impact]}
        if launch:
            # Shared flights (tools/content/weapon_fx.py) by id; the ants' own by key.
            pres['launchEffect'] = launch if launch.startswith('effect.') else FX[launch]
        d['presentation'] = pres
        write(rel, ordered(d))


def write_race():
    race = load('races/ants.json')
    race['description'] = ('One body, ten thousand legs: a swarm of cheap, fast castes that travels underground between its '
                           'mounds, with mutated monsters at the top of the colony.')
    setup = race['startingSetup']
    setup['hero'] = {'default': QUEEN, 'choices': [QUEEN, ALCHEMIST, GENERAL], 'offset': setup['hero']['offset']}
    race['roster'] = [WORKER, RAIDER, NEEDLER, BURSTER, MAJOR, HONEYPOT, ALATE, BULLET, HERALD, GARDENER, REAVER,
                      QUEEN, ALCHEMIST, GENERAL, SPITTER, u('venom-spitter'), u('civilian'), u('youngling'),
                      NEST, GREAT_NEST, CHAMBER, MOUND, BURROW, WORKS, GARDEN, SCENT_HALL, GALLERY, CRADLE, SPIRE,
                      VENOM_SPIRE, ROYAL, SIPHON]
    write('races/ants.json', race)


RETIRED = ['units/ants/amber-adept.json', 'units/ants/hunter.json', 'units/ants/rootweaver.json',
           'units/ants/silkwarden.json', 'units/ants/heroes/rootbreaker.json', 'buildings/ants/forester.json',
           'research/ants/broadheads.json', 'research/ants/campaign-harness.json', 'research/ants/driving-spear.json',
           'research/ants/reinforced-carapace.json', 'research/ants/saturating-shells.json']
RETIRED_ABILITIES = ['ants/amber-adept', 'ants/banneret', 'ants/nectarist', 'ants/queensguard', 'ants/rootweaver',
                     'ants/silkwarden', 'ants/thornblade', 'heroes/marshal', 'heroes/rootbreaker']


def main():
    for rel in RETIRED:
        remove(rel)
    for folder in RETIRED_ABILITIES:
        path = BASE + 'abilities/' + folder
        if os.path.isdir(path):
            for f in os.listdir(path):
                os.remove(os.path.join(path, f))
            os.rmdir(path)
    build_effects()
    unit_abilities()
    hero_abilities()
    research = write_research()
    builds = [NEST, MOUND, BURROW, WORKS, GARDEN, SCENT_HALL, GALLERY, CRADLE, SPIRE, ROYAL, SIPHON]
    write_units(builds)
    write_buildings(research)
    weapon_effects()
    write_race()
    print(len(WRITTEN), 'files')


main()
