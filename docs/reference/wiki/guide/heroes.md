# Heroes, abilities and inventory

The **[Ant Marshal](/units/unit-ants-marshal)** starts with the colony, one unspent skill point, and room to grow through **eleven levels**. His unit page contains the generated XP and stat table. Choose his opening ability before the first fight.

## Experience and levels

Cumulative experience for levels 1–11 is **0, 160, 400, 720, 1,120, 1,600, 2,160, 2,800, 3,520, 4,320, 5,200**. Rewards come from the defeated unit's combat level; heroes use a separate reward table based on their actual current level. Workers and temporary split bodies award none. Summoned units award half the normal reward. Buildings, natural summon expiry and friendly kills do not grant XP.

The killing side shares one reward pool among eligible living allied heroes within **10 C** of the defeated unit. If none are nearby, eligible living heroes belonging to the killing player receive it globally. Enemies do not receive XP for standing nearby. Integer remainders are assigned in stable entity order. Heroes already at the applicable cap do not take a share.

Neutral creeps grant **100%, 90%, 80%, 70%, 60%** of a share to heroes at levels 1–5. They can take a hero all the way to **level 6**, but never provide progress toward level 7. Overflow is discarded. Defeating player-controlled enemies continues progression to level 11. A colony with exactly one rostered hero gains a **15% / 30%** XP bonus at Hall tiers 2 / 3; fallen and paid queued heroes still count toward that roster. Mission level caps may impose a lower ceiling.

Leveling adds the increases in maximum health and mana to current pools, preserving wounds and mana spent. It does not fully heal the hero. Living heroes regenerate continuously; fractional recovery is saved. The Marshal starts at 1.45 HP and 0.76 mana per second and reaches 2.95 HP and 1.51 mana per second at level 11.

## Learn an ability

Open **Learn Ability** to spend a point. You receive one at creation and one per level. The three regular abilities each have three ranks, unlocked at hero levels **1 / 3 / 5**. The ultimate has two ranks, unlocked at **5 / 8**. Eleven points complete the whole kit; points can be saved for later.

- **Faultline (Q):** a travelling ground fracture deals **70 / 110 / 150** spell damage and briefly stuns each ground enemy it crosses. The stun lasts **0.4 / 0.5 / 0.6 seconds**, halved against heroes. Costs 45 mana; 9-second cooldown.
- **Rally the Colony (passive):** grants the Marshal and nearby allied units **8 / 12 / 16%** movement speed and **10 / 15 / 20%** attack speed within **4 C**. Always active once learned, with no mana cost or cooldown. Only the strongest copy applies. The bonus ends outside its radius or when its source dies.
- **Iron Carapace (E):** reduces incoming damage by **20 / 30 / 40%** after ordinary defenses and reflects **15 / 25 / 35%** of ordinary melee damage actually suffered. Lasts six seconds; movement and casting remain available. Costs 40 mana; 14-second cooldown.
- **Crownfall (R):** after a visible 0.8-second windup, deals **180 / 260** spell damage and stuns enemies for **1 / 1.4 seconds**, halved against heroes. Allies in the impact area receive **100 / 160** shielding for six seconds. Radius **1.75 / 2.25 C**; up to sixteen targets per side. Costs **110 / 140** mana; **60 / 54-second** cooldown.

These are published spell definitions with separate editable effects and original icons, organized under **Spells → Heroes → Marshal** in the Spell & Effect Studio. Balance values are an initial playtest baseline.

## Aim before committing

Faultline and Crownfall target ground. Aim the line or circle, then click a valid visible position within **6 C**. Escape or right-click cancels targeting. Carapace casts immediately on the Marshal. Rally activates automatically when learned; its rotating marching crest marks the source and subtle rising motes mark allied recipients.

## Chests and equipment

Clear a camp, then right-click its dropped chest with a hero. The hero moves within pickup range and places the item into a free inventory slot. A full inventory prevents pickup; nothing is discarded automatically.

Equipment provides passive bonuses while carried. Consumables are spent on activation. Right-click an inventory item to drop it deliberately. The [item pages](/items/) list effects, and the [loot tables](/guide/loot) list weighted chances.

## Death is not the end

A fallen hero retains **items, experience and learned abilities**. An **[Amber Sanctuary](/buildings/building-ants-sanctuary)** revives the same hero for **200 + 50 × (level − 1) amber**, taking **30 + 5 × (level − 1) seconds**. He returns with full derived health and mana. Cooldowns keep their deadlines. Blocked exits delay the return; destroying the Sanctuary does not erase the fallen hero.

Hall tier 2 permits a second distinct hero and tier 3 a third. The current faction ships only the Marshal, so extra hero choices become available when additional hero definitions are added. Dead heroes continue reserving their identity and roster slot.
