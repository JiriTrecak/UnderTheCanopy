"""Writes the beetle race: units, heroes, buildings, research, abilities, effects, race.

Run from anywhere: python3 tools/content/beetles/gen.py (writes this repository's base/). Every number lives here, so balance passes edit one
place and regenerate.
"""
import sys
sys.path.insert(0, __import__('os').path.dirname(__import__('os').path.abspath(__file__)))
from gen_common import *
import gen_effects
import json

S = 40  # ticks per second
import os
HP_SCALE = float(os.environ.get('HP_SCALE', '1.3'))
DMG_SCALE = float(os.environ.get('DMG_SCALE', '1.3'))
COST_SCALE = float(os.environ.get('COST_SCALE', '1'))
HEROES = ('hornbreaker', 'forgemaster', 'coilkeeper')

# ----- stand-in art (the integrator swaps in asset.models.units.beetles-<unit> etc.) ------
UNIT_ART = {
    'worker': ('asset.models.units.beetles-tinker', 'asset.portraits.units.beetles-worker'),
    'shieldbearer': ('asset.models.units.beetles-shieldbearer', 'asset.portraits.units.beetles-shieldbearer'),
    'bolter': ('asset.models.units.beetles-bolter', 'asset.portraits.units.beetles-bolter'),
    'breacher': ('asset.models.units.beetles-breacher', 'asset.portraits.units.beetles-breacher'),
    'arcwright': ('asset.models.units.beetles-arcwright', 'asset.portraits.units.beetles-arcwright'),
    'mender': ('asset.models.units.beetles-mender', 'asset.portraits.units.beetles-mender'),
    'warden': ('asset.models.units.beetles-warden', 'asset.portraits.units.beetles-warden'),
    'bombard': ('asset.models.units.beetles-bombard', 'asset.portraits.units.beetles-bombard'),
    'steamwing': ('asset.models.units.beetles-steamwing', 'asset.portraits.units.beetles-steamwing'),
    'juggernaut': ('asset.models.units.beetles-juggernaut', 'asset.portraits.units.beetles-juggernaut'),
    'hornbreaker': ('asset.models.units.beetles-hornbreaker', 'asset.portraits.units.beetles-hornbreaker'),
    'forgemaster': ('asset.models.units.beetles-forgemaster', 'asset.portraits.units.beetles-forgemaster'),
    'coilkeeper': ('asset.models.units.beetles-coilkeeper', 'asset.portraits.units.beetles-coilkeeper'),
    # Stand-ins: no model of their own yet.
    'bolt-turret': ('asset.models.units.beetles-bolt-turret', 'asset.portraits.buildings.beetles-hornwatch'),
    'hornwatch-gunner': ('asset.models.units.beetles-bolter', 'asset.portraits.units.beetles-bolter'),
    'arc-coil': ('asset.models.units.beetles-arcwright', 'asset.portraits.units.beetles-arcwright'),
}
BUILDING_ART = {
    'bastion': ('asset.models.buildings.beetles-bastion', 'asset.portraits.buildings.beetles-bastion', 1.0),
    'great-bastion': ('asset.models.buildings.beetles-great-bastion', 'asset.portraits.buildings.beetles-great-bastion', 1.0),
    'elder-bastion': ('asset.models.buildings.beetles-elder-bastion', 'asset.portraits.buildings.beetles-elder-bastion', 1.0),
    'brood-den': ('asset.models.buildings.beetles-brood-den', 'asset.portraits.buildings.beetles-brood-den', 1.0),
    'war-lodge': ('asset.models.buildings.beetles-war-lodge', 'asset.portraits.buildings.beetles-war-lodge', 1.0),
    'shell-forge': ('asset.models.buildings.beetles-shell-forge', 'asset.portraits.buildings.beetles-shell-forge', 1.0),
    'alembic': ('asset.models.buildings.beetles-alembic', 'asset.portraits.buildings.beetles-alembic', 1.0),
    'engine-yard': ('asset.models.buildings.beetles-engine-yard', 'asset.portraits.buildings.beetles-engine-yard', 1.0),
    'hornwatch': ('asset.models.buildings.beetles-hornwatch', 'asset.portraits.buildings.beetles-hornwatch', 1.0),
    # Stand-in: the Hornwatch until the Arc Tower has its own model.
    'arc-tower': ('asset.models.buildings.beetles-arc-tower', 'asset.portraits.buildings.beetles-hornwatch', 1.0),
    'ancestor-shrine': ('asset.models.buildings.beetles-ancestor-shrine', 'asset.portraits.buildings.beetles-ancestor-shrine', 1.0),
}
ICON = {
    'lock-shields': 'asset.icons.ability-beetles-shieldbearer-lock-shields',
    'armor-breaker': 'asset.icons.ability-beetles-breacher-armor-breaker',
    'arc-surge': 'asset.icons.ability-beetles-arcwright-arc',
    'static-field': 'asset.icons.ability-beetles-arcwright-static-field',
    'mend': 'asset.icons.ability-beetles-mender-patch',
    'field-repair': 'asset.icons.ability-beetles-mender-repair',
    'patch-up': 'asset.icons.ability-beetles-tinker-patch-up',
    'barrier-field': 'asset.icons.ability-beetles-warden-field',
    'bulwark-pulse': 'asset.icons.ability-beetles-warden-bulwark-pulse',
    'flak-beam': 'asset.icons.ability-beetles-steamwing-flak-beam',
    'broadside': 'asset.icons.ability-beetles-juggernaut-broadside',
    'furnace-plating': 'asset.icons.ability-beetles-juggernaut-furnace-plating',
    'bolt-turret': 'asset.icons.ability-beetles-forgemaster-bolt-turret',
    'overclock': 'asset.icons.ability-beetles-forgemaster-overclock',
    'field-forge': 'asset.icons.ability-beetles-forgemaster-field-forge',
    'forgefall': 'asset.icons.ability-beetles-forgemaster-forgefall',
    'arc-lash': 'asset.icons.ability-beetles-coilkeeper-arc-lash',
    'overload': 'asset.icons.ability-beetles-coilkeeper-overload',
    'recharge': 'asset.icons.ability-beetles-coilkeeper-recharge',
    'unbottled': 'asset.icons.ability-beetles-coilkeeper-unbottled',
}
RESEARCH_ICON = {
    # Levelled research: one icon per level, plain iron (I), brass (II), substance glass (III).
    'weapon-edges': [f'asset.icons.research-beetles-weapon-edges-{n}' for n in (1, 2, 3)],
    'bolt-calibre': [f'asset.icons.research-beetles-bolt-calibre-{n}' for n in (1, 2, 3)],
    'plating': [f'asset.icons.research-beetles-plating-{n}' for n in (1, 2, 3)],
    'interlocking-shields': 'asset.icons.research-beetles-interlocking-shields',
    'drum-magazines': 'asset.icons.research-beetles-drum-magazines',
    'heavy-pistons': 'asset.icons.research-beetles-heavy-pistons',
    'adept': 'asset.icons.research-beetles-specialist-training-adept',
    'master': 'asset.icons.research-beetles-specialist-training-master',
    'fragmentation-shells': 'asset.icons.research-beetles-fragmentation-shells',
    'riveted-hulls': [f'asset.icons.research-beetles-riveted-hulls-{n}' for n in (1, 2)],
    'afterburners': 'asset.icons.research-beetles-afterburners',
    'pressure-mains': 'asset.icons.research-beetles-pressure-mains',
}

BUILDING_FX = {
    "hornwatch": [
        {
            "effect": "effect.buildings.hornwatch-canister",
            "state": "work",
            "socket": "socket_canister"
        },
        {
            "effect": "effect.buildings.hornwatch-muzzle",
            "event": "fire",
            "socket": "socket_muzzle"
        }
    ],
    "ancestor-shrine": [
        {
            "effect": "effect.buildings.ancestor-shrine-light",
            "state": "work",
            "socket": "socket_shell"
        },
        {
            "effect": "effect.buildings.ancestor-shrine-motes",
            "state": "work",
            "socket": "socket_dais"
        },
        {
            "effect": "effect.buildings.ancestor-shrine-crackle",
            "state": "work",
            "socket": "socket_orb"
        },
        {
            "effect": "effect.buildings.ancestor-shrine-crackle",
            "state": "work",
            "socket": "socket_emitter_l"
        },
        {
            "effect": "effect.buildings.ancestor-shrine-crackle",
            "state": "work",
            "socket": "socket_emitter_r"
        },
        {
            "effect": "effect.buildings.ancestor-shrine-spawn",
            "event": "flash",
            "socket": "socket_dais"
        }
    ],
    "engine-yard": [
        {
            "effect": "effect.buildings.engine-yard-magic",
            "state": "work",
            "socket": "socket_tank"
        },
        {
            "effect": "effect.buildings.engine-yard-steam",
            "state": "work",
            "socket": "socket_steam"
        },
        {
            "effect": "effect.buildings.engine-yard-sparks",
            "event": "weld",
            "socket": "socket_weld"
        },
        {
            "effect": "effect.buildings.engine-yard-release",
            "event": "release",
            "socket": "socket_hangar"
        }
    ],
    "war-lodge": [
        {
            "effect": "effect.buildings.war-lodge-steam",
            "state": "work",
            "socket": "socket_tower_l"
        },
        {
            "effect": "effect.buildings.war-lodge-steam",
            "state": "work",
            "socket": "socket_tower_r"
        },
        {
            "effect": "effect.buildings.war-lodge-clang",
            "event": "clang",
            "socket": "socket_clang"
        },
        {
            "effect": "effect.buildings.war-lodge-gate",
            "event": "gate",
            "socket": "socket_gate"
        }
    ],
    "alembic": [
        {
            "effect": "effect.buildings.alembic-glow",
            "state": "work",
            "socket": "socket_column"
        },
        {
            "effect": "effect.buildings.alembic-motes",
            "state": "work",
            "socket": "socket_column"
        },
        {
            "effect": "effect.buildings.alembic-crackle",
            "state": "work",
            "socket": "socket_spire"
        },
        {
            "effect": "effect.buildings.alembic-steam",
            "state": "work",
            "socket": "socket_tube_l"
        },
        {
            "effect": "effect.buildings.alembic-steam",
            "state": "work",
            "socket": "socket_tube_r"
        },
        {
            "effect": "effect.buildings.alembic-discharge",
            "event": "discharge",
            "socket": "socket_spire"
        },
        {
            "effect": "effect.buildings.alembic-door",
            "event": "discharge",
            "socket": "socket_door"
        }
    ],
    "brood-den": [
        {
            "effect": "effect.buildings.brood-den-steam",
            "state": "work",
            "socket": "socket_vent"
        },
        {
            "effect": "effect.buildings.brood-den-pressure",
            "state": "work",
            "socket": "socket_gauge"
        },
        {
            "effect": "effect.buildings.brood-den-vent",
            "event": "vent",
            "socket": "socket_vent"
        }
    ],
    "bastion": [
        {
            "effect": "effect.buildings.bastion-glow",
            "state": "work",
            "socket": "socket_glow"
        },
        {
            "effect": "effect.buildings.bastion-motes",
            "state": "work",
            "socket": "socket_tank_l"
        },
        {
            "effect": "effect.buildings.bastion-motes",
            "state": "work",
            "socket": "socket_tank_r"
        },
        {
            "effect": "effect.buildings.bastion-steam",
            "state": "work",
            "socket": "socket_tower_l"
        },
        {
            "effect": "effect.buildings.bastion-steam",
            "state": "work",
            "socket": "socket_tower_r"
        },
        {
            "effect": "effect.buildings.bastion-rivet",
            "event": "rivet",
            "socket": "socket_rivet_a"
        },
        {
            "effect": "effect.buildings.bastion-rivet",
            "event": "rivet",
            "socket": "socket_rivet_b"
        },
        {
            "effect": "effect.buildings.bastion-rivet",
            "event": "rivet-2",
            "socket": "socket_rivet_c"
        },
        {
            "effect": "effect.buildings.bastion-rivet",
            "event": "rivet-2",
            "socket": "socket_rivet_d"
        },
        {
            "effect": "effect.buildings.bastion-gate",
            "event": "gate",
            "socket": "socket_gate"
        }
    ],
    "great-bastion": [
        {
            "effect": "effect.buildings.great-bastion-smoke",
            "state": "work",
            "socket": "socket_chimney_a"
        },
        {
            "effect": "effect.buildings.great-bastion-smoke",
            "state": "work",
            "socket": "socket_chimney_b"
        },
        {
            "effect": "effect.buildings.great-bastion-smoke",
            "state": "work",
            "socket": "socket_chimney_c"
        },
        {
            "effect": "effect.buildings.great-bastion-steam",
            "state": "work",
            "socket": "socket_tower_fl"
        },
        {
            "effect": "effect.buildings.great-bastion-steam",
            "state": "work",
            "socket": "socket_tower_bl"
        },
        {
            "effect": "effect.buildings.great-bastion-steam",
            "state": "work",
            "socket": "socket_tower_br"
        },
        {
            "effect": "effect.buildings.great-bastion-steam",
            "state": "work",
            "socket": "socket_tower_fr"
        },
        {
            "effect": "effect.buildings.great-bastion-motes",
            "state": "work",
            "socket": "socket_tank_fl"
        },
        {
            "effect": "effect.buildings.great-bastion-motes",
            "state": "work",
            "socket": "socket_tank_bl"
        },
        {
            "effect": "effect.buildings.great-bastion-motes",
            "state": "work",
            "socket": "socket_tank_br"
        },
        {
            "effect": "effect.buildings.great-bastion-motes",
            "state": "work",
            "socket": "socket_tank_fr"
        },
        {
            "effect": "effect.buildings.great-bastion-light",
            "state": "work",
            "socket": "socket_core"
        },
        {
            "effect": "effect.buildings.great-bastion-vent",
            "event": "vent-fl",
            "socket": "socket_tower_fl"
        },
        {
            "effect": "effect.buildings.great-bastion-vent",
            "event": "vent-bl",
            "socket": "socket_tower_bl"
        },
        {
            "effect": "effect.buildings.great-bastion-vent",
            "event": "vent-br",
            "socket": "socket_tower_br"
        },
        {
            "effect": "effect.buildings.great-bastion-vent",
            "event": "vent-fr",
            "socket": "socket_tower_fr"
        },
        {
            "effect": "effect.buildings.great-bastion-surge",
            "event": "vent-fl",
            "socket": "socket_tank_fl"
        },
        {
            "effect": "effect.buildings.great-bastion-surge",
            "event": "vent-bl",
            "socket": "socket_tank_bl"
        },
        {
            "effect": "effect.buildings.great-bastion-surge",
            "event": "vent-br",
            "socket": "socket_tank_br"
        },
        {
            "effect": "effect.buildings.great-bastion-surge",
            "event": "vent-fr",
            "socket": "socket_tank_fr"
        },
        {
            "effect": "effect.buildings.great-bastion-rivet",
            "event": "rivet",
            "socket": "socket_rivet_a"
        },
        {
            "effect": "effect.buildings.great-bastion-rivet",
            "event": "rivet",
            "socket": "socket_rivet_b"
        },
        {
            "effect": "effect.buildings.great-bastion-rivet",
            "event": "rivet-2",
            "socket": "socket_rivet_c"
        },
        {
            "effect": "effect.buildings.great-bastion-rivet",
            "event": "rivet-2",
            "socket": "socket_rivet_d"
        },
        {
            "effect": "effect.buildings.great-bastion-gate",
            "event": "gate",
            "socket": "socket_gate"
        }
    ],
    "elder-bastion": [
        {
            "effect": "effect.buildings.elder-bastion-light",
            "state": "work",
            "socket": "socket_core"
        },
        {
            "effect": "effect.buildings.elder-bastion-arc-light",
            "state": "work",
            "socket": "socket_crown"
        },
        {
            "effect": "effect.buildings.elder-bastion-column",
            "state": "work",
            "socket": "socket_reactor"
        },
        {
            "effect": "effect.buildings.elder-bastion-crackle",
            "state": "work",
            "socket": "socket_horn_l"
        },
        {
            "effect": "effect.buildings.elder-bastion-crackle",
            "state": "work",
            "socket": "socket_horn_m"
        },
        {
            "effect": "effect.buildings.elder-bastion-crackle",
            "state": "work",
            "socket": "socket_horn_r"
        },
        {
            "effect": "effect.buildings.elder-bastion-steam",
            "state": "work",
            "socket": "socket_tower_fl"
        },
        {
            "effect": "effect.buildings.elder-bastion-steam",
            "state": "work",
            "socket": "socket_tower_bl"
        },
        {
            "effect": "effect.buildings.elder-bastion-steam",
            "state": "work",
            "socket": "socket_tower_br"
        },
        {
            "effect": "effect.buildings.elder-bastion-steam",
            "state": "work",
            "socket": "socket_tower_fr"
        },
        {
            "effect": "effect.buildings.elder-bastion-vent",
            "event": "vent-fl",
            "socket": "socket_tower_fl"
        },
        {
            "effect": "effect.buildings.elder-bastion-vent",
            "event": "vent-bl",
            "socket": "socket_tower_bl"
        },
        {
            "effect": "effect.buildings.elder-bastion-vent",
            "event": "vent-br",
            "socket": "socket_tower_br"
        },
        {
            "effect": "effect.buildings.elder-bastion-vent",
            "event": "vent-fr",
            "socket": "socket_tower_fr"
        },
        {
            "effect": "effect.buildings.elder-bastion-surge",
            "event": "surge",
            "socket": "socket_cap"
        },
        {
            "effect": "effect.buildings.elder-bastion-rivet",
            "event": "rivet",
            "socket": "socket_rivet_a"
        },
        {
            "effect": "effect.buildings.elder-bastion-rivet",
            "event": "rivet",
            "socket": "socket_rivet_c"
        },
        {
            "effect": "effect.buildings.elder-bastion-rivet",
            "event": "rivet-2",
            "socket": "socket_rivet_b"
        },
        {
            "effect": "effect.buildings.elder-bastion-rivet",
            "event": "rivet-2",
            "socket": "socket_rivet_d"
        },
        {
            "effect": "effect.buildings.elder-bastion-gate",
            "event": "gate",
            "socket": "socket_gate"
        },
        {
            "effect": "effect.buildings.elder-bastion-flash",
            "event": "gate",
            "socket": "socket_core"
        }
    ]
}
BUILDING_FX['arc-tower'] = [
    {'effect': 'effect.buildings.hornwatch-canister', 'state': 'work', 'socket': 'socket_coil'},
    {'effect': 'effect.buildings.hornwatch-canister', 'state': 'work', 'socket': 'socket_muzzle'},
]

HALL = 'building.beetles.bastion'
T2 = 'building.beetles.great-bastion'
T3 = 'building.beetles.elder-bastion'


def u(name):
    return 'unit.beetles.' + name


def b(name):
    return 'building.beetles.' + name


def r(name):
    return 'research.beetles.' + name


# ----- abilities ---------------------------------------------------------------------------

def unit_abilities(fx_):
    A = {}

    def put(rel, a):
        A[a['id']] = a
        write('abilities/beetles/' + rel + '.json', a)
        return a['id']

    put('tinker/patch-up', ability(
        'ability.beetles.patch-up', 'Patch Up',
        'Bolt fresh plates onto an allied machine, restoring 60 health. Autocast; Tinkers repair buildings by hand.',
        'targeted', [{}],
        unit_aim(['ally'], 6, filter={'natures': ['mechanical']}),
        cast(10, 8, 15, 200),
        [{'op': 'heal', 'target': 'target', 'amount': 60}],
        ICON['patch-up'], [fx('patch', fx_['patch'], 'healed')],
        animations=anims('build', 'build', 'idle'),
        autocast={'intervalTicks': 20, 'enabledByDefault': True}))

    put('shieldbearer/lock-shields', ability(
        'ability.beetles.lock-shields', 'Lock Shields',
        'Toggle. Plant the tower shield and lock its clamps: the Shieldbearer cannot move and attacks 25% slower, but gains 6 armor and takes 40% less piercing damage. Interlocking Shields raises this to 10 armor and 55% less piercing damage.',
        'targeted', [{'armor': 6, 'pierce': 600}, {'armor': 10, 'pierce': 450}],
        self_aim(), cast(6, 4, 0, 40),
        [{'op': 'status', 'target': 'caster', 'id': 'locked', 'amount': 80, 'polarity': 'positive', 'dispel': False,
          'lifetime': 'instance', 'modifiers': {'rooted': True, 'attackSpeedPermille': -250},
          'rankedModifiers': {'armor': rp('armor')}, 'damageTakenPermille': {'piercing': rp('pierce')}}],
        ICON['lock-shields'],
        [fx('lock', fx_['brace_lock'], 'statusApplied', statusId='locked'),
         fx('brace', fx_['brace'], 'statusApplied', lifetime='status', statusId='locked')],
        persistent={'durationTicks': 144000, 'intervalTicks': 40, 'anchor': 'caster', 'endsWithCaster': True,
                    'upkeepMana': 0, 'toggle': True}))

    put('breacher/armor-breaker', passive(
        'ability.beetles.armor-breaker', 'Armor Breaker',
        'Every drill strike cracks the target open: its armor falls by 3 for 6 seconds.',
        [{'armor': -3}], [], ICON['armor-breaker'],
        [fx('crack', fx_['crack'], 'statusApplied', lifetime='status', statusId='cracked')],
        combatModifiers={'attackBonus': {'status': 'cracked', 'amount': 0, 'manaCost': 0, 'weapon': 'melee',
                                         'blockedBySilence': False}},
        statuses=[{'op': 'status', 'target': 'target', 'id': 'cracked', 'amount': 240, 'polarity': 'negative',
                   'dispel': True, 'rankedModifiers': {'armor': rp('armor')}}]))

    put('arcwright/arc-surge', ability(
        'ability.beetles.arc-surge', 'Arc Surge',
        'Discharge the coil lance into an enemy: a violet arc that jumps to 3/5 enemies, dealing 55/75 spell damage and keeping 80% of it on each jump.',
        'targeted', [{'hits': 3, 'damage': 55}, {'hits': 5, 'damage': 75}],
        unit_aim(['enemy'], 18), cast(10, 8, 80, 440),
        [{'op': 'damage', 'target': 'target', 'amount': rp('damage'), 'damageType': 'spell'}],
        ICON['arc-surge'], [fx('hop', fx_['surge_hop'], 'impact')],
        delivery={'kind': 'chain', 'bounces': rp('hits'), 'radius': 7, 'intervalTicks': 4, 'retentionPermille': 800,
                  'skipFullHealth': False}))

    put('arcwright/static-field', ability(
        'ability.beetles.static-field', 'Static Field',
        'Charge the ground for 5 seconds: enemies in a 4-radius take 10/15 spell damage each second and move 25/35% slower. Unlocked by Specialist Training.',
        'targeted', [{'damage': 10, 'slow': -250}, {'damage': 15, 'slow': -350}],
        point_aim(['enemy'], 18, 4), cast(10, 8, 90, 640),
        [{'op': 'damage', 'target': 'target', 'amount': rp('damage'), 'damageType': 'spell'},
         {'op': 'status', 'target': 'target', 'id': 'static', 'amount': 48, 'polarity': 'negative', 'dispel': True,
          'rankedModifiers': {'moveSpeedPermille': rp('slow')}}],
        ICON['static-field'],
        [fx('zone', fx_['static_zone'], 'released'), fx('shock', fx_['static_hit'], 'damaged')],
        persistent={'durationTicks': 200, 'intervalTicks': 40, 'anchor': 'point', 'endsWithCaster': False,
                    'upkeepMana': 0, 'toggle': False}))

    put('mender/mend', ability(
        'ability.beetles.mend', 'Mend',
        'Inject a wounded ally with pressurised tonic, restoring 110/160 health. Works on beetles and machines alike. Autocast.',
        'targeted', [{'heal': 110}, {'heal': 160}],
        unit_aim(['ally'], 14, allow_self=True), cast(8, 6, 45, 160),
        [{'op': 'heal', 'target': 'target', 'amount': rp('heal')}],
        ICON['mend'], [fx('mend', fx_['mend'], 'healed')],
        autocast={'intervalTicks': 16, 'enabledByDefault': True}))

    put('mender/field-repair', ability(
        'ability.beetles.field-repair', 'Field Repair',
        'Channel the welding torch for 4 seconds on a machine or building, restoring 45/70 health each second. Unlocked by Specialist Training.',
        'targeted', [{'repair': 45}, {'repair': 70}],
        unit_aim(['ally'], 6, includeBuildings=True), cast(6, 6, 60, 480, channel={'waves': 4, 'intervalTicks': 40, 'tetherRange': 8}),
        [{'op': 'heal', 'target': 'target', 'filter': {'natures': ['mechanical']}, 'amount': rp('repair')},
         {'op': 'heal', 'target': 'target', 'filter': {'buildings': True}, 'amount': rp('repair')}],
        ICON['field-repair'], [fx('weld', fx_['weld'], 'healed')],
        animations=anims('cast', 'cast', 'idle') | {'channel': 'cast'}))

    put('warden/barrier-field', ability(
        'ability.beetles.barrier-field', 'Barrier Field',
        'Toggle. Raise the projector pane: enemy missiles fired at the Warden or anyone behind it from the front stop at the pane, 4 units ahead, and shells burst against it. The Warden moves 30% slower and the field costs 2/1 mana each second.',
        'targeted', [{'upkeep': 2}, {'upkeep': 1}],
        self_aim(), cast(8, 4, 20, 80),
        [{'op': 'status', 'target': 'caster', 'id': 'field', 'amount': 80, 'polarity': 'positive', 'dispel': False,
          'lifetime': 'instance', 'modifiers': {'moveSpeedPermille': -300},
          'barrier': {'radius': 4.0, 'arcDegrees': 150.0}}],
        ICON['barrier-field'],
        [fx('raise', fx_['barrier_up'], 'statusApplied', statusId='field'),
         fx('field', fx_['barrier'], 'statusApplied', lifetime='status', statusId='field')],
        persistent={'durationTicks': 144000, 'intervalTicks': 40, 'anchor': 'caster', 'endsWithCaster': True,
                    'upkeepMana': rp('upkeep'), 'toggle': True}))

    put('warden/bulwark-pulse', ability(
        'ability.beetles.bulwark-pulse', 'Bulwark Pulse',
        'Pulse the projector: up to 8 allies within 8 units gain a field shield absorbing 120/180 damage for 10 seconds. Unlocked by Specialist Training.',
        'targeted', [{'shield': 120}, {'shield': 180}],
        self_aim(), cast(8, 6, 90, 800),
        [{'op': 'status', 'target': 'caster', 'query': {'center': 'caster', 'radius': 8, 'relations': ['ally'],
                                                         'allowSelf': True, 'maxTargets': 8, 'order': 'lowest-health'},
          'id': 'bulwark', 'amount': 400, 'polarity': 'positive', 'dispel': True, 'shield': rp('shield'),
          'onShieldDepleted': 'remove'}],
        ICON['bulwark-pulse'],
        [fx('pulse', fx_['bulwark'], 'released', anchor='caster'),
         fx('ward', fx_['ward'], 'statusApplied', lifetime='status', statusId='bulwark')]))

    put('steamwing/flak-beam', passive(
        'ability.beetles.flak-beam', 'Flak Beam',
        'The substance beam is tuned against flyers: +10 damage against air units.',
        [{'bonus': 10}], [], ICON['flak-beam'], [],
        combatModifiers={'attackBonus': {'amount': rp('bonus'), 'manaCost': 0, 'weapon': 'projectile',
                                         'blockedBySilence': False, 'filter': {'locomotion': ['air']}}}))

    put('juggernaut/broadside', ability(
        'ability.beetles.broadside', 'Broadside',
        'Fire both cannons at once into an area: 140 siege damage to everything hostile in a 5-radius.',
        'targeted', [{}],
        point_aim(['enemy'], 24, 5, includeBuildings=True), cast(16, 12, 80, 600),
        [{'op': 'damage', 'target': 'target', 'amount': 140, 'damageType': 'siege'}],
        ICON['broadside'],
        [fx('fire', fx_['broadside_fire'], 'released', anchor='caster'), fx('impact', fx_['broadside'], 'released')],
        animations=anims('attack', 'attack', 'idle')))

    put('juggernaut/furnace-plating', passive(
        'ability.beetles.furnace-plating', 'Furnace Plating',
        'Riveted furnace plates shrug off magic: spell damage taken is reduced by 30%.',
        [{}], [{'op': 'status', 'target': 'caster', 'id': 'plating', 'amount': 1, 'polarity': 'positive',
                'dispel': False, 'damageTakenPermille': {'spell': 700}}],
        ICON['furnace-plating'], [],
        aura={'radius': 1, 'meleeOnly': False}))
    return A


def hero_abilities(fx_):
    A = {}

    def put(rel, a):
        A[a['id']] = a
        write('abilities/heroes/' + rel + '.json', a)
        return a['id']

    put('forgemaster/bolt-turret', ability(
        'ability.forgemaster.bolt-turret', 'Bolt Turret',
        'Unfold a brass repeater turret at a point for 40 seconds. It shoots ground and air units and can be destroyed. Rank 3 deploys two.',
        'targeted', [{'count': 1}, {'count': 1}, {'count': 2}],
        point_aim(['enemy'], 12, 2, visible=True), cast(16, 10, 90, 600),
        [{'op': 'summon', 'target': 'point', 'amount': rp('count'),
          'definition': {'byRank': [u('bolt-turret'), u('bolt-turret-mk2'), u('bolt-turret-mk2')]},
          'durationTicks': 1600, 'replace': False, 'radius': 2, 'maxActive': 4}],
        ICON['bolt-turret'], [fx('deploy', fx_['turret'], 'summoned')]))

    put('forgemaster/overclock', ability(
        'ability.forgemaster.overclock', 'Overclock',
        'Pull the governor on an allied unit or machine: it attacks and moves 40/55/70% faster for 8 seconds.',
        'targeted', [{'boost': 400}, {'boost': 550}, {'boost': 700}],
        unit_aim(['ally'], 16, allow_self=True), cast(8, 6, 60, 360),
        [{'op': 'status', 'target': 'target', 'id': 'overclocked', 'amount': 320, 'polarity': 'positive', 'dispel': True,
          'rankedModifiers': {'attackSpeedPermille': rp('boost'), 'moveSpeedPermille': rp('boost')}}],
        ICON['overclock'],
        [fx('burst', fx_['overclock_on'], 'statusApplied', statusId='overclocked'),
         fx('heat', fx_['overclock'], 'statusApplied', lifetime='status', statusId='overclocked')]))

    put('forgemaster/field-forge', passive(
        'ability.forgemaster.field-forge', 'Field Forge',
        'Allies within 10 units gain 2/3/4 armor. Every second, allied machines and buildings within 10 units are repaired for 6/10/14 health.',
        [{'armor': 2, 'repair': 6}, {'armor': 3, 'repair': 10}, {'armor': 4, 'repair': 14}],
        [{'op': 'status', 'target': 'target', 'id': 'forged', 'amount': 1, 'polarity': 'positive', 'dispel': False,
          'rankedModifiers': {'armor': rp('armor')}},
         {'op': 'status', 'target': 'caster', 'id': 'forge', 'amount': 1, 'polarity': 'positive', 'dispel': False}],
        ICON['field-forge'],
        [fx('forge', fx_['forge'], 'statusApplied', lifetime='status', statusId='forge'),
         fx('mend', fx_['forge_mend'], 'healed')],
        aura={'radius': 10, 'meleeOnly': False},
        triggers=[{'id': 'repair', 'event': 'interval', 'intervalTicks': 40, 'blockedBySilence': False, 'operations': [
            {'op': 'heal', 'target': 'target', 'query': {'center': 'caster', 'radius': 10, 'relations': ['ally'],
                                                        'allowSelf': False, 'maxTargets': 12},
             'filter': {'natures': ['mechanical']}, 'amount': rp('repair')},
            {'op': 'heal', 'target': 'target', 'query': {'center': 'caster', 'radius': 10, 'relations': ['ally'],
                                                        'includeBuildings': True, 'maxTargets': 6},
             'filter': {'buildings': True}, 'amount': rp('repair')}]}]))

    put('forgemaster/forgefall', ability(
        'ability.forgemaster.forgefall', 'Forgefall',
        'Call down a crucible. After a 1.5-second warning circle it slams into a 5-radius area: 220/340 spell damage and a 1.5-second stun (half on heroes), then a pool of molten metal that burns enemies standing in it for 20/32 spell damage each half second for 6 seconds.',
        'targeted', [{'damage': 220, 'burn': 20}, {'damage': 340, 'burn': 32}],
        point_aim(['enemy'], 22, 5, maxTargets=24, includeBuildings=True), cast(18, 10, 175, 3200),
        [{'op': 'damage', 'target': 'target', 'amount': rp('damage'), 'damageType': 'spell'},
         {'op': 'status', 'target': 'target', 'id': 'slammed', 'amount': 60, 'heroDuration': 30, 'polarity': 'negative',
          'dispel': True, 'stun': True}],
        ICON['forgefall'],
        [fx('warning', fx_['forgefall_warn'], 'released'),
         fx('impact', fx_['forgefall_hit'], 'impact'),
         fx('pool', fx_['forgefall_pool'], 'impact'),
         fx('burn', fx_['burn'], 'damaged')],
        persistent={'durationTicks': 240, 'intervalTicks': 20, 'anchor': 'point', 'endsWithCaster': False,
                    'upkeepMana': 0, 'toggle': False, 'delayTicks': 60,
                    'pulse': [{'op': 'damage', 'target': 'target', 'amount': rp('burn'), 'damageType': 'spell'}]}))

    put('coilkeeper/arc-lash', ability(
        'ability.coilkeeper.arc-lash', 'Arc Lash',
        'Violet lightning leaps between up to 5 enemies, dealing 75/105/135 spell damage and losing 15% on each jump.',
        'targeted', [{'damage': 75}, {'damage': 105}, {'damage': 135}],
        unit_aim(['enemy'], 18), cast(10, 8, 90, 400),
        [{'op': 'damage', 'target': 'target', 'amount': rp('damage'), 'damageType': 'spell'}],
        ICON['arc-lash'], [fx('hop', fx_['lash'], 'impact')],
        delivery={'kind': 'chain', 'bounces': 5, 'radius': 8, 'intervalTicks': 5, 'retentionPermille': 850,
                  'skipFullHealth': False}))

    put('coilkeeper/overload', ability(
        'ability.coilkeeper.overload', 'Overload',
        "Detonate an enemy's mana: drain up to 60/100/140 of it and deal 45/70/95 plus the drained amount as spell damage to every enemy within 4 units of the target.",
        'targeted', [{'drain': 60, 'blast': 45}, {'drain': 100, 'blast': 70}, {'drain': 140, 'blast': 95}],
        unit_aim(['enemy'], 16), cast(10, 8, 75, 520),
        [{'op': 'drain', 'target': 'target', 'resource': 'mana', 'amount': rp('drain'), 'restoreCaster': False,
          'damageType': 'spell', 'damagePerDrainedPermille': 0, 'record': 'burnt'},
         {'op': 'damage', 'target': 'target', 'query': {'center': 'target', 'radius': 4, 'relations': ['enemy'],
                                                         'includeBuildings': False, 'maxTargets': 12},
          'amount': rp('blast'), 'damageType': 'spell',
          'scale': {'source': 'result', 'id': 'burnt', 'stat': 'applied', 'permille': 1000}}],
        ICON['overload'],
        [fx('overload', fx_['overload'], 'released'), fx('shrapnel', fx_['overload_hit'], 'damaged')]))

    put('coilkeeper/recharge', ability(
        'ability.coilkeeper.recharge', 'Recharge',
        'Pour substance into allies in a 6-radius: they regain 40/70/100 mana and gain a shield absorbing 80/130/180 damage for 8 seconds.',
        'targeted', [{'mana': 40, 'shield': 80}, {'mana': 70, 'shield': 130}, {'mana': 100, 'shield': 180}],
        point_aim(['ally'], 18, 6, maxTargets=16), cast(10, 8, 90, 640),
        [{'op': 'mana', 'target': 'target', 'amount': rp('mana')},
         {'op': 'status', 'target': 'target', 'id': 'charged', 'amount': 320, 'polarity': 'positive', 'dispel': True,
          'shield': rp('shield'), 'onShieldDepleted': 'remove'}],
        ICON['recharge'],
        [fx('flow', fx_['recharge'], 'released'),
         fx('shield', fx_['recharge_shield'], 'statusApplied', lifetime='status', statusId='charged')]))

    put('coilkeeper/unbottled', ability(
        'ability.coilkeeper.unbottled', 'Unbottled',
        'Shatter the lantern. For 6 seconds a violet storm stands around the Coilkeeper: enemies within 14 units are dragged towards her, enemies within 7 are lifted off the ground and stunned, and arcs strike up to 6 enemies for 16/24 spell damage five times a second. Allies inside are shielded for 220/320 damage.',
        'targeted', [{'shield': 220, 'arc': 16, 'blast': 60}, {'shield': 320, 'arc': 24, 'blast': 90}],
        self_aim(), cast(14, 8, 200, 3600),
        [{'op': 'damage', 'target': 'caster', 'query': {'center': 'caster', 'radius': 10, 'relations': ['enemy'],
                                                         'maxTargets': 24},
          'amount': rp('blast'), 'damageType': 'spell'},
         {'op': 'status', 'target': 'caster', 'query': {'center': 'caster', 'radius': 12, 'relations': ['ally'],
                                                         'allowSelf': True, 'maxTargets': 24},
          'id': 'sheltered', 'amount': 480, 'polarity': 'positive', 'dispel': False, 'lifetime': 'instance',
          'shield': rp('shield'), 'onShieldDepleted': 'remove'}],
        ICON['unbottled'],
        [fx('shatter', fx_['shatter'], 'released', anchor='caster'),
         fx('storm', fx_['storm'], 'released', anchor='caster'),
         fx('arc', fx_['storm_arc'], 'damaged'),
         fx('lifted', fx_['lifted'], 'statusApplied', lifetime='status', statusId='lifted'),
         fx('shelter', fx_['sheltered'], 'statusApplied', lifetime='status', statusId='sheltered')],
        persistent={'durationTicks': 240, 'intervalTicks': 8, 'anchor': 'caster', 'endsWithCaster': True,
                    'upkeepMana': 0, 'toggle': False,
                    'pulse': [
                        {'op': 'pull', 'target': 'caster', 'query': {'center': 'caster', 'radius': 14,
                                                                     'relations': ['enemy'], 'maxTargets': 32},
                         'amount': 60, 'toward': 'caster', 'minDistance': 2.5},
                        {'op': 'status', 'target': 'caster', 'query': {'center': 'caster', 'radius': 7,
                                                                       'relations': ['enemy'], 'maxTargets': 16},
                         'filter': {'heroes': False}, 'id': 'lifted', 'amount': 20, 'polarity': 'negative',
                         'dispel': False, 'lifetime': 'instance', 'stun': True,
                         'form': {'movement': {'locomotion': 'air', 'flightHeight': 2.5}}},
                        {'op': 'status', 'target': 'caster', 'query': {'center': 'caster', 'radius': 7,
                                                                       'relations': ['enemy'], 'maxTargets': 4},
                         'filter': {'heroes': True}, 'id': 'heldHero', 'amount': 20, 'polarity': 'negative',
                         'dispel': False, 'lifetime': 'instance', 'stun': True},
                        {'op': 'damage', 'target': 'caster', 'query': {'center': 'caster', 'radius': 9,
                                                                       'relations': ['enemy'], 'maxTargets': 6,
                                                                       'order': 'lowest-health'},
                         'amount': rp('arc'), 'damageType': 'spell'}]}))
    return A


# ----- units -------------------------------------------------------------------------------

def weapon(damage, kind, range_, cooldown, aggro, windup, recovery, buffer=0.6, **extra):
    c = {'damage': damage, 'damageType': kind, 'range': range_, 'cooldownTicks': cooldown, 'aggroRange': aggro,
         'attack': {'windupTicks': windup, 'recoveryTicks': recovery, 'rangeBuffer': buffer}}
    c.update(extra)
    return c


def unit(name, title, description, body, combat, speed, creation, supply, dims, scale, *, level=1, requires=None,
         nature=None, abilities=None, presentation=None, selection='army', sets=('behavior-set.ground-army',),
         extra_behaviors=None, vision=32, max_owned=None, movement_extra=None, top=None):
    model, icon = UNIT_ART[name]
    d = {'id': u(name), 'name': title, 'description': description, 'model': model, 'icon': icon}
    if selection:
        d['selectionClass'] = selection
    if selection == 'army' and name not in HEROES:
        body = dict(body, maxHp=int(round(body['maxHp'] * HP_SCALE / 10) * 10))
        if combat:
            combat = dict(combat, damage=max(1, int(round(combat['damage'] * DMG_SCALE))))
        if creation:
            creation = dict(creation, items=[dict(i, amount=int(round(i['amount'] * COST_SCALE / 5) * 5)) for i in creation['items']])
    d['body'] = body
    d['vision'] = vision
    d['behaviorSets'] = list(sets)
    beh = {}
    if combat:
        beh['combat'] = combat
    if speed is not None:
        beh['movement'] = {'speed': speed} | (movement_extra or {})
    if abilities:
        beh['abilities'] = abilities
    if extra_behaviors:
        beh.update(extra_behaviors)
    d['behaviors'] = beh
    if creation:
        d['creation'] = creation
    if requires:
        d['requires'] = requires
    d['level'] = level
    d['supplyCost'] = supply
    if max_owned:
        d['maxOwned'] = max_owned
    if nature:
        d['unitNature'] = nature
    d['dimensions'] = {'radius': dims[0], 'height': dims[1], 'formationSpacing': dims[2]}
    d['modelScale'] = scale
    if presentation:
        d['presentation'] = presentation
    if top:
        d.update(top)
    return d


def train(amber, substance, seconds):
    return {'method': 'train', 'items': cost(amber, substance), 'workTicks': int(seconds * S)}


def body(hp, armor, kind):
    return {'maxHp': hp, 'armor': armor, 'armorType': kind}


def caster(mana, regen, bindings):
    return {'maxMana': mana, 'manaRegenPerSecond': regen, 'bindings': bindings}


def hero_levels(hp, hp_step, dmg, dmg_step, armor, mana, mana_step, cooldown, cd_floor, hregen, mregen):
    xp = [0, 160, 400, 720, 1120, 1600, 2160, 2800, 3520, 4320, 5200]
    rows = []
    for i, e in enumerate(xp):
        rows.append({'experience': e, 'maxHp': hp + hp_step * i, 'damage': dmg + round(dmg_step * i),
                     'armor': armor + i // 2, 'maxMana': mana + mana_step * i,
                     'cooldownTicks': max(cd_floor, cooldown - 2 * i),
                     'healthRegenPerSecond': round(hregen + 0.15 * i, 2),
                     'manaRegenPerSecond': round(mregen + 0.08 * i, 2)})
    return rows


def write_units(W, fx_):
    units = {}

    def put(d):
        units[d['id']] = d
        rel = 'units/beetles/' + ('heroes/' if d.get('hero') else '') + d['id'].split('.')[-1] + '.json'
        write(rel, d)

    infantry = (1.75, 4.8, 3.75)
    bolter_fx = {'projectile': W['bolt'], 'projectileSocket': 'socket_projectile', 'launchEffect': W['muzzle'],
                 'impactEffect': W['bolt_hit']}

    put(unit('worker', 'Tinker',
             'Gathers amber and substance, builds and repairs beetle buildings, and patches machines with Patch Up. Uses 1 supply.',
             body(280, 1, 'light'), None, 11, train(100, 0, 15), 1, (1.5, 4.8, 3.25), 2.4,
             selection='worker', sets=('behavior-set.worker',),
             abilities=caster(60, 0.6, [binding('patch-up', 'ability.beetles.patch-up', 1, 'Q', 1,
                                                preference='wounded-ally', interval=20)]),
             extra_behaviors={'work': {'carryCapacity': 10, 'harvests': ['item.amber', 'item.root'], 'builds': [
                 HALL, b('brood-den'), b('war-lodge'), b('shell-forge'), b('hornwatch'), b('ancestor-shrine'),
                 b('alembic'), b('engine-yard'), b('substance-well')]}},
             vision=28, movement_extra={'walkSpeed': 6}))

    put(unit('shieldbearer', 'Shieldbearer',
             'Front-line tank behind a riveted tower shield, swinging a piston hammer. Lock Shields braces it in place for heavy armor. Slow and stubborn; uses 2 supply.',
             body(520, 2, 'heavy'), weapon(15, 'melee', 0.6, 60, 20, 14, 12), 10, train(185, 0, 18), 2, infantry,
             2.6, abilities=caster(10, 0, [binding('lock-shields', 'ability.beetles.lock-shields', 1, 'Q', 1,
                                                    threat=7, interval=20)]),
             presentation={'impactEffect': W['bash']}))

    put(unit('bolter', 'Bolter',
             'Multi-barrel pressure repeater fed from a back canister: rapid bolts against ground and air. Uses 2 supply.',
             body(420, 0, 'light'), weapon(11, 'piercing', 17, 40, 22, 10, 10, 0.75, projectile={'speed': 40}),
             10, train(180, 0, 18), 2, infantry, 2.5, presentation=bolter_fx))

    put(unit('breacher', 'Breacher',
             'Siege drill arm and clamp. Breaks buildings and armor, and charges in from range. Every strike cracks armor. Requires a Great Bastion; uses 3 supply.',
             body(760, 3, 'heavy'),
             weapon(28, 'siege', 0.8, 72, 20, 16, 14,
                    charge={'minRange': 10, 'maxRange': 28, 'cooldownTicks': 400, 'durationTicks': 100,
                            'speedPermille': 1900, 'damagePermille': 1500}),
             10, train(235, 25, 22), 3, (2.0, 5.6, 4.4), 2.9, level=2, requires=[T2],
             abilities=caster(10, 0, [binding('armor-breaker', 'ability.beetles.armor-breaker', 1, 'Q', 1,
                                              controls=('player', 'ai', 'scenario'))]),
             presentation={'impactEffect': W['drill'], 'cameraAnchor': {'height': 1.6, 'forward': 0.2, 'distance': 7}}))

    put(unit('arcwright', 'Arcwright',
             'Specialist with a coil lance. Its arcs jump between enemies; casts Arc Surge, and Static Field after Specialist Training. Requires a Great Bastion; uses 3 supply.',
             body(400, 1, 'light'),
             weapon(10, 'spell', 15, 64, 20, 16, 12, 0.75, projectile={'speed': 60},
                    chain={'bounces': 1, 'radius': 5.0, 'retentionPermille': 500}),
             10, train(200, 30, 22), 3, infantry, 2.7, level=2, requires=[T2],
             abilities=caster(220, 1.0, [
                 binding('arc-surge', 'ability.beetles.arc-surge', 1, 'Q', 1),
                 binding('static-field', 'ability.beetles.static-field', 0, 'W', 2)]),
             presentation={'projectileSocket': 'socket_projectile', 'launchEffect': W['arc'],
                           'impactEffect': W['arc_hit'], 'castContact': 0.5}))

    put(unit('mender', 'Mender',
             'Field medic with a pressure syringe-gun and a welding torch. Mends beetles and machines; Field Repair after Specialist Training. Requires a Great Bastion; uses 2 supply.',
             body(440, 1, 'light'), weapon(8, 'piercing', 14, 60, 18, 14, 10, 0.75, projectile={'speed': 34}),
             10, train(170, 25, 20), 2, infantry, 2.4, level=2, requires=[T2],
             abilities=caster(240, 1.2, [
                 binding('mend', 'ability.beetles.mend', 1, 'Q', 1, preference='wounded-ally', interval=16),
                 binding('field-repair', 'ability.beetles.field-repair', 0, 'W', 2, preference='wounded-ally',
                         interval=40)]),
             presentation={'projectileSocket': 'socket_projectile', 'launchEffect': W['syringe'],
                           'impactEffect': W['syringe_hit'], 'castContact': 0.36}))

    put(unit('warden', 'Warden',
             'Carries a barrier projector: a curved violet pane that stops enemy missiles and shells from the front. Bulwark Pulse after Specialist Training. Requires a Great Bastion; uses 3 supply.',
             body(640, 2, 'heavy'), weapon(13, 'spell', 6, 56, 18, 12, 10, 0.75, projectile={'speed': 70}),
             10, train(215, 30, 22), 3, (1.8, 5.2, 4.0), 2.6, level=2, requires=[T2],
             abilities=caster(200, 1.2, [
                 binding('barrier-field', 'ability.beetles.barrier-field', 1, 'Q', 1, threat=26),
                 binding('bulwark-pulse', 'ability.beetles.bulwark-pulse', 0, 'W', 2, preference='wounded-ally')]),
             presentation={'projectileSocket': 'socket_projectile', 'launchEffect': W['zap'],
                           'impactEffect': W['arc_hit'], 'castContact': 0.37}))

    put(unit('bombard', 'Bombard',
             'Six-legged mortar crawler with a crewman. Long-range splash that wrecks clumps and buildings, but it cannot fire at anything within 7 units. Requires a Great Bastion; uses 3 supply.',
             body(640, 2, 'heavy'),
             weapon(56, 'siege', 30, 140, 32, 24, 20, 0.75, minRange=7.0,
                    shell={'flightTicks': 46, 'radius': 2.5, 'slowPermille': 0, 'slowTicks': 0}),
             8, train(260, 40, 25), 3, (2.2, 7.0, 5.0), 1.4, level=2, requires=[T2], nature='mechanical',
             presentation={'projectile': 'effect.units.bombard-shell', 'projectileSocket': 'socket_muzzle',
                           'impactEffect': 'effect.units.bombard-impact',
                           'effects': [{'effect': 'effect.units.bombard-launch', 'event': 'release',
                                        'socket': 'socket_muzzle'}]}))

    put(unit('steamwing', 'Steamwing',
             'Boiler-winged flyer: a scout and anti-air harasser whose chin emitter fires violet bolts at ground and air. Flak Beam adds damage against flyers. Requires a Great Bastion; uses 3 supply.',
             body(420, 1, 'light'),
             weapon(14, 'piercing', 12, 48, 22, 10, 10, 0.75, projectile={'speed': 70}, targets=['ground', 'air']),
             16, train(215, 30, 22), 3, (2.2, 5.5, 5.5), 2.2, level=2, requires=[T2], nature='mechanical',
             movement_extra={'locomotion': 'air', 'flightHeight': 7},
             abilities=caster(10, 0, [binding('flak-beam', 'ability.beetles.flak-beam', 1, 'Q', 1)]),
             presentation={'projectile': 'effect.beetles.steamwing-bolt', 'projectileSocket': 'socket_projectile',
                           'impactEffect': 'effect.beetles.steamwing-impact',
                           'effects': [{'effect': 'effect.beetles.steamwing-muzzle', 'event': 'release', 'socket': 'socket_projectile'},
                                       {'effect': 'effect.beetles.steamwing-exhaust', 'state': 'walk', 'socket': 'socket_exhaust_b'},
                                       {'effect': 'effect.beetles.steamwing-exhaust', 'state': 'run', 'socket': 'socket_exhaust_b'}]}, vision=36))

    put(unit('juggernaut', 'Juggernaut',
             'Ultimate walking fortress: six legs, a violet furnace window, two cannons and a crew. Fires Broadside; Furnace Plating turns aside 30% of spell damage. One at a time. Requires an Elder Bastion; uses 8 supply.',
             body(3000, 6, 'heavy'),
             weapon(62, 'siege', 22, 96, 26, 20, 18, 0.75,
                    shell={'flightTicks': 30, 'radius': 2.8, 'slowPermille': 0, 'slowTicks': 0}),
             7, train(600, 150, 50), 8, (2.5, 13.0, 6.5), 1.9, level=4, requires=[T3], nature='mechanical',
             max_owned=1,
             abilities=caster(160, 0.8, [
                 binding('broadside', 'ability.beetles.broadside', 1, 'Q', 1, interval=40),
                 binding('furnace-plating', 'ability.beetles.furnace-plating', 1, 'W', 2)]),
             presentation={'projectile': 'effect.units.juggernaut-shell', 'projectileSocket': 'socket_projectile',
                           'impactEffect': 'effect.units.juggernaut-impact',
                           'effects': [{'effect': 'effect.units.juggernaut-muzzle', 'event': 'release', 'socket': 'socket_muzzle_l'},
                                       {'effect': 'effect.units.juggernaut-muzzle', 'event': 'release', 'socket': 'socket_muzzle_r'},
                                       {'effect': 'effect.units.juggernaut-step', 'event': 'step', 'socket': 'socket_foot_fl'},
                                       {'effect': 'effect.units.juggernaut-step', 'event': 'step', 'socket': 'socket_foot_mr'},
                                       {'effect': 'effect.units.juggernaut-step', 'event': 'step', 'socket': 'socket_foot_bl'},
                                       {'effect': 'effect.units.juggernaut-furnace', 'state': 'always', 'socket': 'socket_furnace'},
                                       {'effect': 'effect.units.juggernaut-steam', 'state': 'always', 'socket': 'socket_steam'},
                                       {'effect': 'effect.units.juggernaut-crash', 'event': 'crash'}],
                           'cameraAnchor': {'height': 3.0, 'forward': 0.2, 'distance': 12}}, vision=34))

    # Summons and crews: no supply, no player control.
    turret_body = (1.5, 4.6, 3.4)
    for name, title, hp, dmg, scale in (('bolt-turret', 'Bolt Turret', 320, 16, 2.4),
                                        ('bolt-turret-mk2', 'Bolt Turret Mk II', 420, 22, 2.75)):
        d = unit('bolt-turret', title,
                 'A brass repeater turret deployed by the Forgemaster. Shoots ground and air units; it runs down after 40 seconds.',
                 body(hp, 3, 'heavy'),
                 weapon(dmg, 'piercing', 16, 36, 18, 8, 8, 0.75, projectile={'speed': 44}, targets=['ground', 'air']),
                 None, None, 0, turret_body, scale, nature='mechanical', sets=(), selection=None, vision=24,
                 presentation=bolter_fx)
        d['id'] = u(name)
        put(d)
    put(unit('hornwatch-gunner', 'Hornwatch Gunner',
             'The repeater crew of a Hornwatch.', body(100, 0, 'light'),
             weapon(24, 'piercing', 20, 44, 22, 8, 8, 0.75, projectile={'speed': 44}, targets=['ground', 'air']),
             None, None, 0, (0.8, 3.0, 2.0), 1.6, nature='mechanical', sets=(), selection=None, vision=30,
             presentation=bolter_fx, top={'selectable': False}))
    put(unit('arc-coil', 'Arc Coil',
             'The substance coil of an Arc Tower.', body(100, 0, 'light'),
             weapon(30, 'spell', 20, 60, 22, 10, 8, 0.75, projectile={'speed': 70}, targets=['ground', 'air'],
                    chain={'bounces': 3, 'radius': 6.0, 'retentionPermille': 700}),
             None, None, 0, (0.8, 3.0, 2.0), 1.6, nature='mechanical', sets=(), selection=None, vision=30,
             presentation={'launchEffect': W['arc'], 'impactEffect': W['arc_hit']}, top={'selectable': False}))

    # Heroes
    hero_dims = (3.0, 9.6, 6.5)

    def hero(name, title, description, levels, combat, speed, scale, mana_regen, bindings, presentation, top=None):
        d = unit(name, title, description, body(levels[0]['maxHp'], levels[0]['armor'], 'hero'), combat, speed,
                 train(425, 75, 55), 4, hero_dims, scale,
                 abilities=caster(levels[0]['maxMana'], mana_regen, bindings), presentation=presentation, vision=36,
                 extra_behaviors={'progression': {'experienceRadius': 40, 'levels': levels},
                                  'inventory': {'slots': 4, 'pickupRange': 1.5}})
        d['hero'] = True
        if top:
            d.update(top)
        put(d)

    hero('hornbreaker', 'Hornbreaker',
         'Beetle warlord with a Hercules horn and a great hammer. Charges into battle, overwhelms a chosen enemy, steadies nearby allies and becomes a Colossus.',
         hero_levels(900, 90, 44, 4.5, 4, 200, 20, 80, 64, 1.8, 0.75),
         weapon(44, 'hero', 0.8, 80, 20, 18, 14), 11, 3.84, 0.75,
         [binding('horn-rush', 'ability.hornbreaker.horn-rush', 0, 'Q', 1, levels=[1, 3, 5], icon='asset.icons.ability-beetles-hornbreaker-horn-rush'),
          binding('overwhelm', 'ability.hornbreaker.overwhelm', 0, 'W', 2, levels=[1, 3, 5], icon='asset.icons.ability-beetles-hornbreaker-overwhelm'),
          binding('steadfast', 'ability.hornbreaker.steadfast', 0, 'E', 3, levels=[1, 3, 5], icon='asset.icons.ability-beetles-hornbreaker-steadfast'),
          binding('colossus', 'ability.hornbreaker.colossus', 0, 'R', 4, levels=[5, 8], icon='asset.icons.ability-beetles-hornbreaker-colossus')],
         {'impactEffect': W['bash'], 'castContact': 0.47})
    hero('forgemaster', 'Forgemaster',
         'Engineer hero with a smokestack helm and a back arm rig. Deploys Bolt Turrets, Overclocks allies, keeps a Field Forge burning around him and calls down Forgefall.',
         hero_levels(760, 75, 27, 3.5, 3, 240, 25, 78, 64, 1.4, 0.85),
         weapon(27, 'hero', 13, 78, 20, 18, 14, 0.75, projectile={'speed': 40}), 11, 4.4, 0.85,
         [binding('bolt-turret', 'ability.forgemaster.bolt-turret', 0, 'Q', 1, levels=[1, 3, 5]),
          binding('overclock', 'ability.forgemaster.overclock', 0, 'W', 2, levels=[1, 3, 5], threat=14),
          binding('field-forge', 'ability.forgemaster.field-forge', 0, 'E', 3, levels=[1, 3, 5]),
          binding('forgefall', 'ability.forgemaster.forgefall', 0, 'R', 4, levels=[5, 8])],
         {'projectile': W['bolt'], 'projectileSocket': 'socket_projectile', 'launchEffect': W['muzzle'],
          'impactEffect': W['bolt_hit'], 'castContact': 0.54},
         top={'displayOrder': 2})
    hero('coilkeeper', 'Coilkeeper',
         'Substance savant with a coil halo and a lantern-staff. Lashes enemies with chained lightning, Overloads their mana, Recharges allies and, once, lets the substance loose: Unbottled.',
         hero_levels(640, 68, 23, 3.0, 1, 300, 32, 72, 60, 1.2, 1.05),
         weapon(23, 'hero', 16, 72, 20, 18, 12, 0.75, projectile={'speed': 60}), 11, 3.35, 1.05,
         [binding('arc-lash', 'ability.coilkeeper.arc-lash', 0, 'Q', 1, levels=[1, 3, 5]),
          binding('overload', 'ability.coilkeeper.overload', 0, 'W', 2, levels=[1, 3, 5]),
          binding('recharge', 'ability.coilkeeper.recharge', 0, 'E', 3, levels=[1, 3, 5], preference='wounded-ally'),
          binding('unbottled', 'ability.coilkeeper.unbottled', 0, 'R', 4, levels=[5, 8], threat=12)],
         {'projectileSocket': 'socket_projectile', 'launchEffect': W['arc'], 'impactEffect': W['arc_hit']},
         top={'displayOrder': 3})
    units[u('hornbreaker')]['displayOrder'] = 1
    write('units/beetles/heroes/hornbreaker.json', units[u('hornbreaker')])
    return units


# ----- buildings ---------------------------------------------------------------------------

def construct(amber, substance, seconds):
    return {'method': 'construct', 'items': cost(amber, substance), 'workTicks': int(seconds * S),
            'workAnimation': 'build'}


def building(name, title, description, hp, armor, size, creation, *, behaviors=None, requires=None, supply=None,
             vision=16, category='category.build', priority=50, upgrade=None, level=1, extra=None, command=True,
             network=None, power=True):
    model, icon, scale = BUILDING_ART[name]
    d = {'id': b(name), 'name': title, 'description': description, 'model': model, 'icon': icon,
         'body': {'maxHp': hp, 'armor': armor, 'armorType': 'structure'}, 'vision': vision,
         'footprint': {'width': size, 'depth': size}, 'entrance': {'x': 0.5, 'y': size / 2 + 0.5}}
    if creation:
        d['creation'] = creation
    if requires:
        d['requires'] = requires
    d['behaviorSets'] = ['behavior-set.player-building']
    d['behaviors'] = behaviors or {}
    if category:
        d['category'] = category
    d['level'] = level
    if upgrade:
        d['upgrade'] = upgrade
    if supply:
        d['supplyProvided'] = supply
    if network:
        d['network'] = {'role': network[0], 'radius': network[1]}
    if power:
        d['power'] = 'required'
    d['modelScale'] = scale
    if command and category:
        d['command'] = {'category': category, 'priority': priority}
    if name in BUILDING_FX:
        d['presentation'] = {'effects': BUILDING_FX[name]}
    if extra:
        d.update(extra)
    return d


def bills(capacity):
    return {'capacity': capacity, 'accepts': ['item.amber', 'item.root']}


def write_buildings(research_ids):
    out = {}

    def put(d):
        out[d['id']] = d
        write('buildings/beetles/' + d['id'].split('.')[-1] + '.json', d)

    hall_storage = {'capacity': 100000, 'accepts': ['item.amber'], 'dropoff': True,
                    'gathering': {'item': 'item.amber', 'radius': 40}}
    tinkers = {'mode': 'queued', 'outputs': [u('worker')], 'workerSlots': 0, 'queueCapacity': 6}
    put(building('bastion', 'Bastion',
                 'The beetle colony’s iron-clad main hall: stores goods, trains Tinkers, provides 15 supply and feeds the substance network. Upgrades to the Great Bastion.',
                 2000, 5, 20, construct(450, 0, 90), vision=42, priority=100, supply=15,
                 network=('source', 32), power=False,
                 behaviors={'storage': hall_storage, 'production': tinkers,
                            'research': {'outputs': [research_ids['pressure-mains']], 'queueCapacity': 1}},
                 upgrade={'target': T2, 'items': cost(400, 75), 'workTicks': 80 * S},
                 extra={'heroCapacity': 1}))
    put(building('great-bastion', 'Great Bastion',
                 'Hall tier 2: unlocks the Alembic, the Engine Yard and tier-2 troops, a second hero and level II research. Researches Pressure Mains; upgrades to the Elder Bastion.',
                 2700, 6, 20, None, vision=42, supply=15, level=2, category=None,
                 network=('source', 32), power=False,
                 behaviors={'storage': hall_storage, 'production': tinkers,
                            'research': {'outputs': [research_ids['pressure-mains']], 'queueCapacity': 1}},
                 upgrade={'target': T3, 'items': cost(600, 150), 'workTicks': 100 * S},
                 extra={'heroCapacity': 2}))
    put(building('elder-bastion', 'Elder Bastion',
                 'Hall tier 3, upgraded once an Engine Yard stands: unlocks the Juggernaut, level III research, Specialist Training Master, the Arc Tower and a third hero.',
                 3500, 7, 20, None, vision=42, supply=15, level=3, category=None, requires=[b('engine-yard')],
                 network=('source', 32), power=False,
                 behaviors={'storage': hall_storage, 'production': tinkers,
                            'research': {'outputs': [research_ids['pressure-mains']], 'queueCapacity': 1}},
                 extra={'heroCapacity': 3}))
    put(building('brood-den', 'Brood Den',
                 'An iron-banded shell nursery: provides 8 supply and extends the substance network with a generous radius.',
                 700, 3, 8, construct(110, 0, 25), priority=90, supply=8, network=('node', 28)))
    put(building('war-lodge', 'War Lodge',
                 'Iron-clad barracks for Shieldbearers, Bolters and Breachers. Researches Interlocking Shields, Drum Magazines and Heavy Pistons.',
                 1400, 5, 16, construct(220, 0, 50), priority=80,
                 behaviors={'storage': bills(800),
                            'production': {'mode': 'queued', 'outputs': [u('shieldbearer'), u('bolter'), u('breacher')],
                                           'workerSlots': 0, 'queueCapacity': 6},
                            'research': {'outputs': [research_ids[k] for k in ('interlocking-shields', 'drum-magazines', 'heavy-pistons')],
                                         'queueCapacity': 3}}))
    forge = building('shell-forge', 'Shell Forge',
                     'Forges weapons and plating for the colony: Weapon Edges, Bolt Calibre and Plating, three levels each. Level II needs a Great Bastion, level III an Elder Bastion.',
                     1250, 5, 16, construct(170, 0, 40), priority=30,
                     behaviors={'research': {'outputs': research_ids['forge'], 'queueCapacity': 3}})
    forge['presentation'] = {'effects': [
        {'effect': 'effect.buildings.forge-fire', 'state': 'work', 'socket': 'socket_furnace'},
        {'effect': 'effect.buildings.forge-magic', 'state': 'work', 'socket': 'socket_tank'},
        {'effect': 'effect.buildings.forge-smoke', 'state': 'work', 'socket': 'socket_chimney_a'},
        {'effect': 'effect.buildings.forge-smoke', 'state': 'work', 'socket': 'socket_chimney_b'},
        {'effect': 'effect.buildings.forge-smoke', 'state': 'work', 'socket': 'socket_chimney_c'},
        {'effect': 'effect.buildings.forge-sparks', 'event': 'strike', 'socket': 'socket_anvil'}]}
    put(forge)
    put(building('alembic', 'Alembic',
                 'Distillery of the specialists who handle substance through devices: trains Arcwrights, Menders and Wardens and researches Specialist Training. Requires a Great Bastion.',
                 1150, 4, 12, construct(200, 40, 45), requires=[T2], category='category.build-advanced', priority=90,
                 behaviors={'storage': bills(800),
                            'production': {'mode': 'queued', 'outputs': [u('arcwright'), u('mender'), u('warden')],
                                           'workerSlots': 0, 'queueCapacity': 6},
                            'research': {'outputs': [research_ids['adept'], research_ids['master']], 'queueCapacity': 2}}))
    put(building('engine-yard', 'Engine Yard',
                 'Machine works: builds Bombards, Steamwings and the Juggernaut, and researches Fragmentation Shells, Riveted Hulls and Afterburners. Requires a Great Bastion.',
                 1600, 5, 16, construct(250, 50, 55), requires=[T2], category='category.build-advanced', priority=80,
                 behaviors={'storage': bills(1200),
                            'production': {'mode': 'queued', 'outputs': [u('bombard'), u('steamwing'), u('juggernaut')],
                                           'workerSlots': 0, 'queueCapacity': 6},
                            'research': {'outputs': [research_ids[k] for k in ('fragmentation-shells', 'riveted-hulls-1', 'riveted-hulls-2', 'afterburners')],
                                         'queueCapacity': 3}}))
    garrison = lambda crew: {'capacity': 1, 'accepts': [], 'height': 7.7, 'lookoutRadius': 0.6, 'crew': crew}
    put(building('hornwatch', 'Hornwatch',
                 'Armored tower crewed by its own repeater gunner: shoots ground and air units for 24 damage, far harder than other races’ towers. Needs the substance network. Upgrades to the Arc Tower at tier 3.',
                 850, 4, 8, construct(150, 0, 35), category='category.build-advanced', priority=100, vision=26,
                 upgrade={'target': b('arc-tower'), 'items': cost(125, 50), 'workTicks': 40 * S},
                 extra={'garrison': garrison(u('hornwatch-gunner'))}))
    put(building('arc-tower', 'Arc Tower',
                 'A Hornwatch refitted with a substance coil: its arcs deal 30 spell damage and jump to three more enemies. Requires an Elder Bastion.',
                 1100, 5, 8, None, requires=[T3], category=None, vision=28, level=3,
                 extra={'garrison': garrison(u('arc-coil'))}))
    put(building('ancestor-shrine', 'Ancestor Shrine',
                 'Where the colony’s heroes are called and reconstituted: trains the Hornbreaker, the Forgemaster and the Coilkeeper, and revives the fallen.',
                 1150, 5, 12, construct(200, 0, 40), priority=70,
                 behaviors={'storage': bills(100000),
                            'production': {'mode': 'queued', 'outputs': [u('hornbreaker'), u('forgemaster'), u('coilkeeper')],
                                           'workerSlots': 0, 'queueCapacity': 3},
                            'revival': {'items': cost(220), 'itemsPerLevel': cost(50), 'workTicks': 30 * S,
                                        'workTicksPerLevel': 5 * S, 'queueCapacity': 3}}))
    return out


# ----- research ----------------------------------------------------------------------------

def write_research():
    ids = {}

    def put(key, name, title, description, icon, priority, items, seconds, effects, requires=None):
        rid = r(name)
        d = {'id': rid, 'name': title, 'description': description, 'icon': icon, 'priority': priority}
        if requires:
            d['requires'] = requires
        d['items'] = items
        d['workTicks'] = int(seconds * S)
        d['effects'] = effects
        write('research/beetles/' + name + '.json', d)
        ids[key] = rid
        return rid

    melee = [u('shieldbearer'), u('breacher')]
    ranged = [u('bolter'), u('steamwing'), u('hornwatch-gunner'), u('bolt-turret'), u('bolt-turret-mk2')]
    plated = [u(n) for n in ('shieldbearer', 'bolter', 'breacher', 'arcwright', 'mender', 'warden', 'bombard',
                             'steamwing', 'juggernaut')]
    forge = []
    tiers = [(None, 'I', 0, 150, 0, 40), (T2, 'II', 1, 200, 50, 50), (T3, 'III', 2, 260, 100, 60)]
    for key, label, units, effect_of, icon, prio in (
            ('weapon-edges', 'Weapon Edges', melee, lambda: {'damagePermille': 120}, RESEARCH_ICON['weapon-edges'], 90),
            ('bolt-calibre', 'Bolt Calibre', ranged, lambda: {'damagePermille': 120}, RESEARCH_ICON['bolt-calibre'], 85),
            ('plating', 'Plating', plated, lambda: {'armor': 2}, RESEARCH_ICON['plating'], 80)):
        prev = None
        for hall, roman, i, amber, substance, seconds in tiers:
            what = {'weapon-edges': 'Shieldbearers and Breachers deal 12% more attack damage',
                    'bolt-calibre': 'Bolters, Steamwings, Hornwatch gunners and Bolt Turrets deal 12% more attack damage',
                    'plating': 'Beetle troops and machines gain 2 armor'}[key]
            gate = {None: '', T2: ' Requires a Great Bastion.', T3: ' Requires an Elder Bastion.'}[hall]
            requires = [x for x in (hall, prev) if x]
            name = f'{key}-{i + 1}'
            rid = put(name, name, f'{label} {roman}', f'{what} (level {roman}; levels add up).{gate}', icon[i],
                      prio - i, cost(amber, substance), seconds, [dict({'units': units}, **effect_of())],
                      requires=requires or None)
            forge.append(rid)
            prev = rid
    ids['forge'] = forge
    put('interlocking-shields', 'interlocking-shields', 'Interlocking Shields',
        'Locked shields interlock: Lock Shields gives 10 armor instead of 6 and cuts piercing damage by 55% instead of 40%.',
        RESEARCH_ICON['interlocking-shields'], 70, cost(125, 20), 40,
        [{'units': [u('shieldbearer')], 'abilityRanks': {'lock-shields': 2}}])
    put('drum-magazines', 'drum-magazines', 'Drum Magazines',
        'Bolters attack 25% faster.', RESEARCH_ICON['drum-magazines'], 75, cost(150, 25), 45,
        [{'units': [u('bolter')], 'attackSpeedPermille': 250}])
    put('heavy-pistons', 'heavy-pistons', 'Heavy Pistons',
        'Breachers can charge every 5 seconds instead of 10. Requires a Great Bastion.',
        RESEARCH_ICON['heavy-pistons'], 65, cost(150, 30), 45,
        [{'units': [u('breacher')], 'chargeCooldownPermille': 500}], requires=[T2])
    specialists = [u('arcwright'), u('mender'), u('warden')]
    put('adept', 'specialist-training-adept', 'Specialist Training: Adept',
        'Arcwrights learn Static Field, Menders Field Repair and Wardens Bulwark Pulse.',
        RESEARCH_ICON['adept'], 80, cost(150, 50), 50,
        [{'units': [u('arcwright')], 'abilityRanks': {'static-field': 1}},
         {'units': [u('mender')], 'abilityRanks': {'field-repair': 1}},
         {'units': [u('warden')], 'abilityRanks': {'bulwark-pulse': 1}}])
    put('master', 'specialist-training-master', 'Specialist Training: Master',
        'Every specialist ability reaches its second rank: stronger Arc Surge and Static Field, Mend and Field Repair, a cheaper Barrier Field and a stronger Bulwark Pulse. Requires an Elder Bastion.',
        RESEARCH_ICON['master'], 70, cost(250, 100), 70,
        [{'units': [u('arcwright')], 'abilityRanks': {'arc-surge': 2, 'static-field': 2}},
         {'units': [u('mender')], 'abilityRanks': {'mend': 2, 'field-repair': 2}},
         {'units': [u('warden')], 'abilityRanks': {'barrier-field': 2, 'bulwark-pulse': 2}}],
        requires=[T3, r('specialist-training-adept')])
    put('fragmentation-shells', 'fragmentation-shells', 'Fragmentation Shells',
        'Bombard and Juggernaut shells burst wider: blast radius grows to 3.5 units and they slow survivors by 25% for 2 seconds.',
        RESEARCH_ICON['fragmentation-shells'], 75, cost(175, 50), 50,
        [{'units': [u('bombard'), u('juggernaut')], 'splashRadius': 3.5, 'splashSlowPermille': 250}])
    machines_hp = lambda: [{'units': [u('bombard')], 'maxHp': 100}, {'units': [u('steamwing')], 'maxHp': 80},
                           {'units': [u('juggernaut')], 'maxHp': 450}]
    put('riveted-hulls-1', 'riveted-hulls-1', 'Riveted Hulls I',
        'Machines gain health: Bombards +100, Steamwings +80, the Juggernaut +450.',
        RESEARCH_ICON['riveted-hulls'][0], 70, cost(150, 30), 45, machines_hp())
    put('riveted-hulls-2', 'riveted-hulls-2', 'Riveted Hulls II',
        'Machines gain the same again: Bombards +100, Steamwings +80, the Juggernaut +450. Requires an Elder Bastion.',
        RESEARCH_ICON['riveted-hulls'][1], 69, cost(225, 75), 60, machines_hp(),
        requires=[T3, r('riveted-hulls-1')])
    put('afterburners', 'afterburners', 'Afterburners', 'Steamwings fly 30% faster.',
        RESEARCH_ICON['afterburners'], 60, cost(150, 30), 40,
        [{'units': [u('steamwing')], 'moveSpeedPermille': 300}])
    # The network's radius is built by the network itself (D66); this research is its hook.
    put('pressure-mains', 'pressure-mains', 'Pressure Mains',
        'Raises the pressure in the colony’s mains: the substance network reaches 8 cells further around the Bastion, the Substance Wells and every Brood Den. Requires a Great Bastion.',
        RESEARCH_ICON['pressure-mains'], 40, cost(150, 75), 45,
        [{'units': [HALL, T2, T3, b('brood-den'), b('substance-well')], 'networkRadius': 8}], requires=[T2])
    return ids


def write_race():
    race = {
        'id': 'beetles',
        'name': 'Beetles',
        'description': 'Slow, stubborn and territorial: expensive, durable troops and machines that run on substance they never let touch them.',
        'startingSetup': {
            'fort': HALL,
            'inventory': {'item.amber': 575},
            'units': [{'definition': u('worker'), 'offset': {'x': x, 'y': 14}} for x in (-6, -3, 0, 3, 6)],
            'id': 'setup.beetles',
            'gathering': [{'item': 'item.amber', 'workers': 5, 'radius': 40}],
            'hero': {'default': u('hornbreaker'),
                     'choices': [u('hornbreaker'), u('forgemaster'), u('coilkeeper')],
                     'offset': {'x': 0, 'y': -16}},
        },
        'roster': [u(n) for n in ('worker', 'shieldbearer', 'bolter', 'breacher', 'arcwright', 'mender', 'warden',
                                  'bombard', 'steamwing', 'juggernaut', 'hornbreaker', 'forgemaster', 'coilkeeper',
                                  'bolt-turret', 'bolt-turret-mk2', 'hornwatch-gunner', 'arc-coil')]
                  + [b(n) for n in ('bastion', 'great-bastion', 'elder-bastion', 'brood-den', 'war-lodge',
                                    'shell-forge', 'alembic', 'engine-yard', 'hornwatch', 'arc-tower',
                                    'ancestor-shrine', 'substance-well')],
        'construction': {'scaffold': 'asset.models.construction.beetles', 'padding': 0.4, 'revealAt': 0.95,
                         'retractUntil': 0.985, 'dustEffect': 'effect.construction.dust', 'dustIntervalTicks': 8},
    }
    write('races/beetles.json', race)


def tune_hornbreaker():
    # The Hornbreaker keeps his kit; Steadfast steadies the line as well as mending it.
    path = BASE + 'abilities/heroes/hornbreaker/steadfast.json'
    d = json.load(open(path))
    d['ranks'] = [{'regen': 2, 'armor': 1}, {'regen': 3, 'armor': 2}, {'regen': 4, 'armor': 3}]
    d['onRelease'][0]['rankedModifiers'] = {'healthRegenPerSecond': rp('regen'), 'armor': rp('armor')}
    d['description'] = ('Nearby allied units regenerate 2/3/4 additional health per second and gain '
                        '1/2/3 armor within four grid cells. Only the strongest copy applies. Ends when '
                        'the Hornbreaker falls or an ally leaves the aura.')
    write('abilities/heroes/hornbreaker/steadfast.json', d)
    path = BASE + 'abilities/heroes/hornbreaker/horn-rush.json'
    d = json.load(open(path))
    for rank, (damage, stun) in zip(d['ranks'], ((90, 40), (140, 60), (190, 80))):
        rank.update({'damage': damage, 'stun': stun, 'cooldown': 400})
    d['description'] = ('Charge a visible ground enemy, respecting terrain and other bodies. On contact deal '
                        '90/140/190 spell damage and stun for 1/1.5/2 seconds. Blocked, interrupted or '
                        'escaped charges do not hit.')
    write('abilities/heroes/hornbreaker/horn-rush.json', d)


def main():
    for old in ('effects/beetles/weapons/mortar-launch.json', 'effects/beetles/weapons/mortar-impact.json',
                'effects/beetles/weapons/cannon-launch.json', 'effects/beetles/weapons/cannon-impact.json',
                'effects/beetles/weapons/steam-beam.json', 'effects/beetles/weapons/steam-beam-impact.json',
                'units/beetles/horn-guard.json', 'units/beetles/stone-slinger.json',
                'research/beetles/broadheads.json', 'research/beetles/campaign-harness.json',
                'research/beetles/reinforced-carapace.json', 'research/beetles/serrated-tools.json'):
        remove(old)
    W = gen_effects.build()
    fx_ = gen_effects.build_abilities() | gen_effects.build_heroes()
    unit_abilities(fx_)
    hero_abilities(fx_)
    research_ids = write_research()
    write_units(W, fx_)
    write_buildings(research_ids)
    write_race()
    tune_hornbreaker()
    print(len(WRITTEN), 'files')


main()
