# Map weather

The Environment panel offers Biome default, Clear, Rain, Snow and Drifting spores. Maps store only an optional `{kind}` in `landscape.environment.weather`. Omission inherits the biome's default. Changing a condition preserves the live preview hour unless a new hour is specified.

Biomes own intensity and horizontal wind through their environment default and optional `weatherLooks` profiles in `src/content/biomes.ts`. Uncustomized kinds use the shared engine weather defaults. Maps and MCP cannot tune these visual parameters. Legacy intensity/wind fields are stripped on load/export. Wind uses world axes in world units per second.

Weather is cosmetic: it does not change gathering, combat, movement, navigation or deterministic simulation state. Snowfall does not accumulate on terrain or change tree materials; choose Frozen Forest for winter assets.

`WeatherLayer` renders at most 768 particles in one camera-local instanced batch. Clear weather hides the batch and skips particle updates. Rain uses short narrow streaks; snow uses camera-facing flakes. Spores use soft round billboards, rise slowly at 0.18 metres/second and wander horizontally within a 1–7 metre band above terrain or water. The layer shares normal terrain depth testing and the game's fog material integration. Editor and gameplay use the same renderer. Maps that omit weather inherit the biome default.

## Mist and sunlight shafts

Biomes also define [volumetric atmosphere](./volumetric-atmosphere.md). Its density field follows weather wind, while rain reduces shaft intensity. It has separate graphics quality settings and remains cosmetic.


Spore validation covers map roundtripping, bounded particle count, clearance above water, slow motion, repeatable visual positions for the same timestamp and disabling the batch. Ground paint renders before water, and weather after it, while normal depth testing still hides particles behind solid geometry.
