-- The autoplay plan's API (docs/campaign.md "Autoplay"), defined in the plan's own
-- environment before the plan runs. Everything ends in one host function, `call`; the steps
-- that take time yield the plan's coroutine until the host says they are done.

local host, yield = ...

local function call(...)
  return host.call(...)
end

-- Starts a step, then hands the turn back every tick until the step is done.
local function block(...)
  call("begin", ...)
  while true do
    local done, a, b = call("poll")
    if done then
      return a, b
    end
    yield()
  end
end

-- Steps: they take time, and the plan goes on when they are done.
function go(x, z, how) return block("go", x, z, how) end
function attack(id) return block("attack", id) end
function pickup(id, unit) return block("pickup", id, unit) end
function wait(seconds) return block("wait", seconds) end
function wait_objective(id, seconds) return block("objective", id, seconds) end
function wait_clock(minutes) return block("clock", minutes) end
function mass(n, seconds) return block("mass", n, seconds) end

function wait_until(test, seconds)
  local deadline = seconds and call("ticks") + math.floor(seconds * 40)
  while not test() do
    if deadline and call("ticks") >= deadline then
      return false
    end
    yield()
  end
  return true
end

-- Standing orders, commands and queries: they answer at once.
local direct = {
  "hero", "keep", "economy", "push", "click", "train", "research", "build", "cast", "use_item", "learn",
  "say", "army", "army_center", "alive", "position", "hp", "objective", "stock", "count", "scene",
  "clock", "level",
}
for i = 1, #direct do
  local name = direct[i]
  _ENV[name] = function(...)
    return call(name, ...)
  end
end
