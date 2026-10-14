# Match chat and HUD feedback

## Playing

Press **Enter** to open match chat. Type a message and press Enter again to send it to everyone; **Escape** closes the input without sending. Player names use their team color and message text is white. Recent messages appear on the left and fade after ten seconds. Opening chat shows the scrollable history (latest 200 messages), with the input below it. History belongs to the current session and is not saved with a match.

While chat is open, game shortcuts, edge scrolling and camera momentum are suspended. The match continues running. In local skirmishes an AI says **gg** once when its objective Mound falls below 15% health, or when a sudden lethal hit defeats it. This is cosmetic; it does not surrender or change the AI's orders. Hosted multiplayer currently has human slots; their messages travel over WebSocket to every connected participant.

## Contracts

- `src/shared/chat/chat.ts` owns the text contract: plain text, 300 characters, no control characters. UI uses DOM text nodes, never HTML.
- Client wire message: `{ type: "chat", text }`. The authenticated server connection supplies name and player slot; clients cannot impersonate a sender.
- Server wire message: `{ type: "chat", message: { name, player, text } }`. Spectator names use neutral gray. Hosted matches accept one message per connection every 500 ms.
- Chat is out-of-band presentation traffic, absent from deterministic command queues, checksums and simulation saves. `Session` connects transport to `GameChat`; UI never imports networking code.

## Command cards

Shortcut labels come from command declarations: a bottom-right icon badge and a tooltip title such as **Faultline (Q)**. A successfully accepted `learnAbility` command returns to the root command card; rejected learning requests leave the menu in place.

Disabled cards keep their explanatory tooltips and respond to clicks with the declared failure reason, without sending an invalid action. Simulation failures and unreachable targets use explicit `error` facts, separated from ordinary progression and ability announcements. Errors appear in red above the bottom HUD for 2.5 seconds, then fade over 0.3 seconds. Repeated errors restart the timer.

Currency definitions have optional `displayOrder` metadata (lower first). Amber = 10, Wood = 20, Root = 30. Resource summaries, observer columns and tooltip costs follow this order. This only affects presentation; economy and recipe ordering are unchanged.
