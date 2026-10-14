// The old TypeScript client in a room on a local match server, using its own lobby
// (`src/net/lobby.ts`) and socket (`src/net/ws.ts`) code, reporting what it sees as JSON
// lines on stdout. `tests/old_client.rs` runs it from the old repository:
//   node --import tsx <this file> <old repository> <http://127.0.0.1:port> <room id>
// The old client always talks to the deployed server, so its URLs are redirected here.
import {join} from 'node:path';

const [oldRepo, base, roomId] = process.argv.slice(2);
if (!oldRepo || !base || !roomId) throw Error('usage: lobby_peer.mts <old repository> <base url> <room id>');
const load = (file: string) => import(join(oldRepo, file));
const say = (event: string, data: object = {}) => console.log(JSON.stringify({event, ...data}));

const {MATCH_HOST} = await load('src/shared/net/endpoint.ts');
const realFetch = globalThis.fetch;
globalThis.fetch = ((url: string | URL | Request, init?: RequestInit) =>
  realFetch(String(url).replace(`https://${MATCH_HOST}`, base), init)) as typeof fetch;

const lobby = await load('src/net/lobby.ts');
const {WebSocketChannel} = await load('src/net/ws.ts');
const {SIMULATION_BUILD} = await load('src/shared/simulationBuild.ts');
const {matchContentIdentity} = await load('src/content/identity.ts');

const joined = await lobby.joinRoom(roomId, {guestName: 'TS', role: 'player'});
say('joined', {you: joined.you});
const url = lobby.matchUrl(joined.room.id, joined.token).replace(`wss://${MATCH_HOST}`, base.replace(/^http/, 'ws'));
const channel = new WebSocketChannel(url);
const identity = await matchContentIdentity();
channel.onMessage((msg: {type: string}) => {
  say('message', {msg});
  // As the old match screen does once its world is loaded.
  if (msg.type === 'start') channel.send({type: 'ready', content: identity, build: SIMULATION_BUILD});
  if (msg.type === 'error') setTimeout(() => process.exit(0), 50);
});
setTimeout(() => {say('timeout'); process.exit(1);}, 30000);
