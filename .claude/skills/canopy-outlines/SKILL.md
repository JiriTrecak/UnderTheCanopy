---
name: canopy-outlines
description: Ideation for Under the Canopy's campaigns in their outlines (D110, D111): what an outline holds field by field, how to read the story around a step before proposing anything, how to pitch distinct takes and develop the one Jiri picks into purpose, pillars, objectives, beats, cast, threads and a tabletop sketch, and how to paint concept pictures (generate_image) and iterate on them visually. Use whenever brainstorming, pitching, outlining or picturing a campaign step, mission, story, chapter or cinematic, before any map is built.
---

# Outlines and ideation

A campaign is designed in its **outlines** before anything is built (D110). Every step of a campaign (chapter, story, mission, cinematic) can have one: `outlines/<step id>.json` in its campaign package. They are working documents: plain words, never text keys, and releases leave them out, pictures and all. A **draft** step has only its outline: it never plays and never ships. The **early access line** is one step in the whole game; releases ship what is before it.

Read before ideating:
- **docs/story.md:** the forest and the substance, the trilogy, the characters, the campaign's step table, how missions are built, and each built mission's design. Its arc is Jiri's; the steps are proposals.
- **docs/campaign.md:** the campaign system (steps, tech, what carries over, campaign AI). Drafts and outlines are under "Drafts and the early access line".
- **docs/decisions.md** (D85, D96, D105, D110, D111): binding.

In the editor, the Campaigns workbench shows a campaign as **Steps** (the list), **Outline** (one step: the one-pager on the left, a mission's beats on the right, and in the middle its tabletop sketch, pages or shots, or its **Pictures**) and **Read-through** (every step of every campaign in one row, a lane per character, the tension curve, the early access line).

## Reading the story around a step

Never ideate from the step alone. Before proposing anything:
1. Know where the user is. The assistant's `<editor-context>` names the campaign, the view, the selected step (its kind, title, id, draft), what its outline holds, and the steps before and after it.
2. `campaign_read_through` gives the whole story in play order, every step with its outline. Narrow it with `campaign` and `from`/`to` (step ids or indexes). Sketches come counted unless `sketches: true`.
3. `campaign_outline {campaign, step}` gives one step's outline whole, with its title and the steps either side.
4. `read_doc docs/story.md` for the arc, the characters and the built missions' designs, which outlines only point to.
5. Look at the art when it matters: `view_image` takes a campaign's outline picture (`file`) or any image asset (`asset`): story pages and mission art under `asset.interface.campaign.<campaign>.…`, portraits and icons. `list_elements {editor: "assets"}` finds them.

Read what comes before and after the step: which threads are open, who is on stage, how tense the neighbours are, which kinds of mission came just before (the read-through warns when two neighbours share a kind).

## Pitching

Ideation is a conversation with Jiri. The arc and every decision are his.
- **Pitch before writing.** Offer two or three takes that differ in what the player does, not only in flavour: a defend against a hero walk against a race against time. Each take is a title, a line or two of pitch, the three pillars (play, character, story) and what makes it memorable: its moment, its secret, its silly thing.
- **Say which you would pick, and why**, in a sentence.
- **Write into the outline only what he chose.** Don't fill outlines with pitches he hasn't seen.
- **Ask in prose**, one or two questions at a time, when something is his to decide.

What makes a good step:
- **It earns its place in the arc:** it opens, carries or closes a thread, grows a character, and raises or releases tension against its neighbours (1 calm to 5 the peak).
- **Kinds vary** from mission to mission (hero walk, base, defend, timed, escort, dungeon, stealth, race, hold points, split forces).
- **Missions follow "How the missions are built"** in docs/story.md:
  - a clear main road with the objective on it;
  - side places worth the detour (a camp with a drop, a side quest, a merchant, a spring);
  - one or two secrets hidden by the map's shape;
  - one easter egg;
  - beats in place, as short letterboxed scenes and lines when things are found.
- **Warcraft III's campaigns are the model.** Short scenes, a character moment in every mission, enemies with a face, a twist the player half saw coming.
- **Tone:**
  - The ant campaign is warm and enchanted: bio-magic, living light, blossoms.
  - Corruption's look is first seen at the fall, its last step. Before it, withered spots may only hint, as omens.
  - Substance is a glowing liquid, blue to violet, never green.
- **Names** must be as memorable as Arthas or Sylvanas: short, singular, their sound fitting the character. Keep the established names (Aurelith, Oleander, Korrath, Tiv, Ashka, Vosk) unless Jiri changes them.
- **The campaign is linear.** There is no branching and no side missions. Heroes carry their level, abilities and items to the next mission. The tech grows step by step.

## Writing the outline

`set_campaign_outline {campaign, step, fields}` sets top-level fields, creating the outline when it has none. It is one undo step, and `null` removes a field. An array field is replaced whole, so read it first and send the full list. The format is `canopy_content::outline::Outline`:

| Field | For | Shape |
|---|---|---|
| `purpose` | every step | What it is for in the campaign |
| `notes` | every step | Free notes and open questions |
| `cast` | every step | `[{who: unit or hero id, presence: on \| mentioned \| joins \| leaves \| open, note}]` |
| `threads` | every step | `[{mark: opens \| carries \| closes, text}]` |
| `tension` | every step | 1 (calm) to 5 (the peak) |
| `images` | every step | The pictures (below); `generate_image` adds them |
| `kinds` | mission | Of: hero-walk, base, defend, timed, escort, dungeon, stealth, race, hold-points, split-forces |
| `pitch` | mission | A line or two |
| `pillars` | mission | `{play, character, story}`: what it plays, whom it grows, what it tells |
| `objectives` | mission | `[{kind: main \| optional \| secret \| easterEgg, text}]` |
| `minutes` | mission | About how long it plays |
| `beats` | mission | `[{phase: before \| during \| after, speaker: unit id, trigger, mode: scene \| line, text}]` |
| `sketch` | mission | `{size, shapes, stamps}`, below |
| `location` | cinematic | Where it happens |
| `madeAs` | cinematic | `scene` (in-engine), `film`, `card` (a story card for now) |
| `shots` | cinematic | `[{framing: wide \| medium \| close \| two-shot \| over…, action, speaker, line, seconds}]` |

A story's pages are its own, in `campaign.json` (`set_campaign_step`), not its outline.

**The cast** names characters by their unit or hero definition (`unit.ants.queen` is Aurelith). Its portrait and name come from there.

**The tabletop sketch** is a map drawn roughly, in map cells `[x, z]` on a square of `size` (a map size, 256 by default; x runs right and z down, as maps are drawn from above):
- `shapes`: `[{brush, points: [[x, z]…]}]`. The brushes are `pen` (a line), `high`, `forest` and `water` (areas: the outline's points in order), `road` (the main road, with an arrow at its end), `path` (a side path, dashed) and `attack` (where enemies come from, dashed, with an arrow).
- `stamps`: `[{mark, x, z, label, count, who}]`. The marks are:
  - `start`, `amber`;
  - `camp` (`count` its size), `hero` (`who`);
  - `objective` (`label` its number), `optional` (`label` its letter), `secret`;
  - `beat` (`label` its number), `shop`, `spring`;
  - `text` (`label`: words on the map).

Draw the road first, then the high ground and forests that shape it, then the places along it and the secrets off it. The beats' numbers match the beats list. A sketch is not the map: building the real one (the Outline's Make map, then skill canopy-maps) waits until Jiri agrees the design.

After writing, call `problems`: the campaign check reads every outline and flags unknown kinds, a tension outside 1–5, an outline of no step, and a picture whose file is missing. Never save unless Jiri asks.

## Pictures

Concept pictures make ideas concrete: the place a mission happens, a character's look, a beat's moment, a chapter's mood, a cinematic's key frames.

**`generate_image {campaign, step, prompt, caption, references, model, size, quality, count}`** paints them with OpenAI's image models:
- It uses the OpenAI key in the assistant's Settings.
- It costs money and takes up to two minutes; the user allows it, once or for the conversation.
- It writes them to the campaign's `outlines/images/` at once and adds them to the step's outline `images` as one undo step. A picture no saved outline names is deleted on save.
- It returns a preview to look at. Look at it, judge it against the idea, and say what works and what doesn't.

**Prompts.** Write the prompt in full, as a painter's brief:
1. The subject and the moment.
2. The composition and where the eye goes, at insect scale.
3. The light and the time of day, and the mood.
4. The style tail. End every prompt with it:

> Painterly but crisp, rich saturated colours in the style of Warcraft III: Reforged key art and cinematic concept art. The world is a forest floor seen at insect scale: moss, colossal roots, ferns, fallen oak leaves, pebbles like boulders. Substance, the forest's magic, is a glowing liquid of blue to violet light, never green. Lanterns and fire are warm amber. No text, no letters, no logos, no UI, no frame, no border.

The races:
- **Ants:** sand-clay spire architecture like a Gaudí termite cathedral, glowing blue-violet oval windows, flowering vines, royal-blue drapes. Ant-like and humanoid units mixed. Aurelith is slender and regal: an amber crown, a royal-blue mantle, dark-red chitin.
- **Beetles:** engineers of glass, coils and engines, who keep substance in glass and never in their bodies.
- **Bees:** a holy order of wax, ritual and gold.

Describe characters by their look, not only their names: the image model doesn't know them.

**Iterating:**
- `count` 2–4 gives takes to choose from; one is the default.
- `references` starts from earlier pictures (files of `outlines/images`, any step's) or the game's art (asset ids). The model then varies or refines them: "the same scene at night", "closer on the Queen", "the same yard in the beetles' colours".
- Keep the captions short and telling ("The yard at dusk, take 2"). The Pictures tab shows them.
- Remove a failed picture with `set_campaign_outline` (send `images` without it).
- **Models:**
  - `gpt-image-2.5-sunburst` (the default) paints best.
  - `gpt-image-2.5-flare` is fast, for quick variations.
  - `gpt-image-2` is the older one.
- **Size and quality:** `1536x1024` landscape is the default (`1024x1536` for a portrait, `2048x1152` for a finished key frame). `quality` is low, medium (the default) or high.
- **Spend with care.** Paint what the conversation needs, a few at a time. Ask before a large batch.

Pictures are for thinking. Story pages, mission art and loading screens are game art, made with skill canopy-campaign ("Story and art") once a design is agreed.

## A session, start to finish

1. Read the context and the read-through. Look at the step's neighbours and their pictures.
2. Pitch two or three takes. Recommend one and ask.
3. Paint a picture or two of the chosen take's key place or moment. Iterate on them with Jiri.
4. Write the outline: purpose, kinds, pitch, pillars, objectives, beats, cast, threads, tension, minutes. Sketch the map.
5. Call `problems`. Say what changed, and what is still open (in `notes`).
6. Leave saving, and building the map, to Jiri's word.
