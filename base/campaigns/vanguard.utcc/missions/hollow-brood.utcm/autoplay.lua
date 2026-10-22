-- The Hollow Brood, played by its plan (docs/campaign.md "Autoplay"): the Queen's column
-- takes the caches and the side places worth the detour, frees the lost patrol, breaks
-- through to the survivors and puts Mother Ashka to rest. Scenes play out; the plan waits.

hero{"queen",
  learn = {"living-light", "cocoon", "one-body"},
  heal = "living-light", heal_below = 0.55,
  shield = "cocoon", shield_below = 0.4,
  potions = {"item.resin-salve", "item.trailkeeper-flask"}, potion_below = 0.35,
}

-- The caches by the road.
attack("cache.salve")
pickup()
attack("cache.dew")
pickup()

-- The frog on the lily pool: ten clicks, a croak, and a Heartseed left behind.
go(116, 296)
click("frog", 10)
wait(3)
pickup()

-- The capling ring, the ford, the dead sentry.
go(134, 298)
go(192, 262)
go(189, 243)

-- The lost patrol in the webs; Kesh's whistle comes with them.
go(150, 208)
go(96, 198)
wait_objective("patrol", 60)
pickup()

-- The old burrow beyond the webs, and its flask.
go(74, 120)
attack("cache.burrow")
pickup()

-- The sick pool, then the sap spring to rest.
go(122, 168)
go(254, 238)
wait(25)

-- The beetle wreck and its crate.
go(304, 292)
attack("crate.husk")
pickup()
go(254, 238)
wait(20)

-- The terrace, and a look into the Moonlit Hollow above it (the shrine's wisp sleeps).
go(198, 110)
go(204, 66, "walk")

-- The rim, the rubble and the survivors behind it.
go(296, 122)
go(330, 136)
attack("rubble")
wait_objective("survivors", 60)
go(334, 160)
wait(30)

-- The Hollow, and Mother Ashka.
go(304, 92)
go(300, 66)
wait_objective("ashka", 240)
