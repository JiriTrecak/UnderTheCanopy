import {COMMAND_DELAY,TICK_MS} from './protocol';

/** Server-owned samples; clients echo an opaque nonce, never a claimed duration. */
export class ConnectionLatency {
  private pending: {id:string;sent:number}|null=null;
  private samples: {rtt:number;at:number}[]=[];
  private lastProbe=-Infinity;
  reset(){this.pending=null;this.samples=[];this.lastProbe=-Infinity;}
  probe(now:number):string|null {
    if(this.pending&&now-this.pending.sent<5000)return null;
    if(this.samples.length>=5&&now-this.lastProbe<2000)return null;
    const id=crypto.randomUUID();
    this.pending={id,sent:now};this.lastProbe=now;
    return id;
  }
  reply(id:string,now:number):boolean {
    if(!this.pending||this.pending.id!==id)return false;
    const elapsed=now-this.pending.sent;this.pending=null;
    if(!Number.isFinite(elapsed)||elapsed<0||elapsed>5000)return false;
    this.samples.push({rtt:elapsed,at:now});this.samples=this.samples.slice(-5);
    return true;
  }
  roundTrip(now:number):number|null {
    const fresh=this.samples.filter(s=>now-s.at<=15000);
    return fresh.length<3?null:Math.ceil(Math.max(...fresh.map(s=>s.rtt)));
  }
}

/** Common frozen pipeline: worst recent player RTT plus one simulation beat. */
export function connectionDelay(roundTrips:readonly (number|null)[]):number {
  const known=roundTrips.filter((n):n is number=>n!==null&&Number.isFinite(n)&&n>=0);
  const measured=Math.max(2,Math.ceil((Math.max(0,...known)+TICK_MS)/TICK_MS));
  const fallback=known.length!==roundTrips.length||!roundTrips.length?COMMAND_DELAY:2;
  return Math.min(40,Math.max(fallback,measured));
}
