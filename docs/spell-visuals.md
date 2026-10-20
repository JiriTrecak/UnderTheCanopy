# Spell visuals

What every spell looks like, and why: one visual idea per spell that says what it does, so
spells read apart at a glance (D78). Warcraft III is the bar: Holy Light is a symbol, a cone and a
pulse; Resurrection an angel and light; Blizzard falling shards. Generic pieces (smoke, dust,
debris, sparks) only support the idea; a disc or ring is used only where the mechanic is a ring.

Each entry: **what it does** → **the idea** (its signature) · supporting layers · reference.
Effects are generated (`tools/content/<race>/…`); the field reference is the canopy-abilities
skill's `presentation.md`.

## Beetles

Machines of brass and iron, substance (violet-blue liquid light) in lanterns and coils, fire
in furnaces, steam from every vent.

### Coilkeeper (substance lightning)

- **Arc Lash** — chain lightning between up to five enemies → **forked violet lightning that
  dances between victims**, each victim crackling white for an instant · a star at each hit, a
  scatter of sparks · Chain Lightning.
- **Overload** — drains the target's mana and detonates it on everyone near → **the mana is
  sucked out**: violet motes spiral up out of the target into an orb that swells and flickers
  above it, then **bursts** in a ring of substance and lightning to the victims · a shrapnel
  spark on each victim · Mana Burn + Thunder Clap.
- **Recharge** — pours substance into allies (mana and a shield) → **a lantern-light rain**:
  violet droplets fall over the area and each ally **fills up**: motes spiral down into it and a
  violet hexagon shell (the `shell` texture) crackles around it · Mass rejuvenation / SC2 Shield Battery.
- **Unbottled** — the lantern shatters into a six-second storm that drags enemies in, lifts
  them and strikes them with arcs, sheltering allies → **a violet tornado**: glass bursts, then
  the ground darkens, spiral arms (`vortex`) turn and motes and haze **orbit inward** around
  the Coilkeeper, each lifted enemy spins in a small violet twister, arcs lash out from the eye · Cyclone + Psionic Storm.

### Forgemaster (forge, brass and fire)

- **Bolt Turret** — unfolds a turret at a point → **assembly**: brass plates and sparks
  **swirl in** and drop onto the spot under a welding fountain, then slam together: dust bursts
  out, steam blasts from the seams, a small brass gear locks ·
  WC3 building birth / SC2 warp-in.
- **Overclock** — a unit attacks and moves much faster → **redline**: on the cast an exhaust
  flame roars up, steam jets out of its sides and sparks spray; while it lasts the unit **sheds a trail of hot sparks and heat**
  behind it as it moves (world space) · Bloodlust / Stimpack.
- **Field Forge** — allies gain armour; machines and buildings are repaired each second → a
  small dark-brass gear turning under each ally with forge sparks floating up; each repair is
  **rivets hammered**: three quick white-hot strikes, each with a spark spray · Devotion Aura.
- **Forgefall** — a red-hot anvil falls, slams, leaves molten metal → done (D76): the falling
  anvil with its fire column, the fire-to-ash blast wave, the molten pool · Inferno.

### Hornbreaker (warlord: horn, hammer, earth and furnace)

- **Horn Rush** — charges an enemy and stuns it → **a charge you can follow**: a dust wall
  kicked up and a furrow scraped in the ground behind him, sparks off the horn; on contact
  **the earth bursts**: dark rocks and clods thrown up and landing, a crack in the ground,
  pale dust rolling out · Shockwave +
  War Stomp.
- **Overwhelm** — melee hits stack a mark that slows → **a crushing hammer blow**: a crescent
  of light, metal chips, a short crack under the target · Bash.
- **Steadfast Aura** — nearby allies regenerate and gain armour → quiet: **a brass shield sigil**
  at each ally's feet and a few warm motes rising · Devotion Aura.
- **Colossus** — becomes a living siege engine → **the furnace ignites**: fire roars up from his
  vents, **steam jets blast out sideways**, the ground under him is scorched and shakes (pale dust
  rolling out, dark stones jumping); while it lasts
  **burning footprints** and embers pour off him · Avatar / Metamorphosis.

### Units

- **Arc Surge** (Arcwright) — smaller chain lightning → violet lightning jumping between
  victims, thinner than Arc Lash.
- **Static Field** (Arcwright) — charged ground that hurts and slows → **a crackling carpet**:
  dark ground, short sparks leap across it in every direction, little stars pop, a violet haze
  clings low · Static
  Link / SC2 Psi Storm (low).
- **Armor Breaker** (Breacher) — drill hits crack armour → **plates break off**: plate fragments
  fly and land; while cracked an orange fracture glints on the unit and flakes keep falling.
- **Broadside** (Juggernaut) — two cannons blast an area → **a walking barrage**: three fire
  blasts in turn, rocks and dirt thrown up, three scorch marks, smoke.
- **Field Repair** (Mender) — welding torch on a machine → **a weld**: a white-hot point, a
  fountain of sparks and **molten drops** falling and landing.
- **Mend** (Mender) — tonic restores health → **tonic bubbles** rising up through the patient,
  a soft blue pulse, steam from the seams.
- **Lock Shields** (Shieldbearer) — plants and locks the shield → **clamps bite**: a steel clang,
  sparks spraying along the ground, dust and steam; while locked a steady blue shield sigil at
  its feet.
- **Patch Up** (Tinker) — bolts plates on → rivet sparks and a glint of a new brass plate.
- **Barrier Field** (Warden) — a projector pane ahead of the Warden → a violet hexagon dome
  (`shell`) snaps up bright and shimmers while held.
- **Bulwark Pulse** (Warden) — shields allies around → the mechanic is a pulse, so **a blue
  field wave** rolls out, and each shielded ally wears a thin blue hexagon shell.
