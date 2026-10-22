-- Founding, played by its plan (docs/campaign.md "Autoplay"): the colony is the skirmish
-- AI's, held to the mission's tech; the plan wakes the sleeping Alate, then sends the army
-- against the ridge burrows and across the root bridge into the den.

hero{"queen"}
economy(true)

-- The sleeping Alate in its flower bed: three clicks and it sneezes.
wait_clock(1.5)
go(156, 214)
click("alate-sleeper", 3)
go(204, 252)

-- The ridge burrows; once they are seen broken, across the root bridge into the den.
local function gone(ids)
  return function()
    for i = 1, #ids do
      if alive(ids[i]) ~= false then return false end
    end
    return true
  end
end
wait_clock(12)
push(300, 100)
wait_clock(15)
push(322, 80)
wait_until(gone({"burrow.ne.1", "burrow.ne.2"}), 9 * 60)
push(110, 120)
go(110, 120)
wait(60)
push(86, 92)
wait_objective("burrows")
