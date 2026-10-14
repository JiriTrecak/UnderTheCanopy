# Ant progression

Gameplay progression is declared in `content/game.json`; current costs, durations, stats and unlocks are generated into the wiki from that registry. Definitions can exist while their art uses a missing-model placeholder.

Native support includes owned-building prerequisites, placement near observed finite resources, specialized drop-offs, paid in-place building upgrades and colony-wide research queues. Upgrades preserve entity identity; research modifies declared capabilities. Cancellation refunds follow the authoritative queue rules, destruction removes unfinished purchases, and completed knowledge persists through saves.

The ant roster includes Great Mound progression, Rootworks, Ironroot Forge, Hunter and Bombardier definitions. Root is a finite gathered resource. Hunters use the native charge behavior; Bombardiers use persistent non-homing shells. See [shell combat](shell-combat.md), the content registry and `src/sim/game/{upgrades,research,charge}.ts` for contracts.

Check prerequisite rejection, funding, cancellation, destruction, duplicate queues, resource depletion, new-unit identity and supply reservations and save continuation when editing this progression. Do not assume that every declared definition is exposed in the current worker menu; action visibility is authored separately.
