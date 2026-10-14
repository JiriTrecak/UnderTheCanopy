/** Original, reproducible spell sound palette. Mono PCM keeps downloads and decoded memory small. */
import {readFile} from 'node:fs/promises';
import {originalPackage,addBytes,publishOriginals} from './original-publication';
const rate=22050,tau=Math.PI*2;
function wav(seconds:number,sample:(t:number,i:number)=>number){
 const frames=Math.round(seconds*rate),b=Buffer.alloc(44+frames*2);
 b.write('RIFF');b.writeUInt32LE(b.length-8,4);b.write('WAVEfmt ',8);b.writeUInt32LE(16,16);b.writeUInt16LE(1,20);b.writeUInt16LE(1,22);b.writeUInt32LE(rate,24);b.writeUInt32LE(rate*2,28);b.writeUInt16LE(2,32);b.writeUInt16LE(16,34);b.write('data',36);b.writeUInt32LE(frames*2,40);
 for(let i=0;i<frames;i++)b.writeInt16LE(Math.round(Math.max(-1,Math.min(1,sample(i/rate,i)))*32767),44+i*2);return b;
}
const noise=(i:number)=>{let n=Math.imul(i+31,0x45d9f3b);n=Math.imul(n^(n>>>16),0x45d9f3b);return ((n^(n>>>16))>>>0)/2147483648-1;};
const clips=[
 {id:'radiant-chime',name:'Radiant Chime',seconds:1.2,sample:(t:number)=>Math.min(1,t/.012)*Math.max(0,1-t/1.2)*Math.exp(-t*3)*(.32*Math.sin(tau*660*t)+.18*Math.sin(tau*990*t)+.12*Math.sin(tau*1320*t)+.08*Math.sin(tau*1760*t))},
 {id:'frost-impact',name:'Frost Impact',seconds:.65,sample:(t:number,i:number)=>Math.min(1,t/.002)*Math.max(0,1-t/.65)*(.28*noise(i)*Math.exp(-t*26)+.2*Math.sin(tau*1870*t)*Math.exp(-t*10)+.12*Math.sin(tau*2833*t)*Math.exp(-t*14))},
 {id:'ward-hum',name:'Ward Hum',seconds:1,sample:(t:number)=>.15*Math.sin(tau*110*t)+.07*Math.sin(tau*165*t)+.04*Math.sin(tau*220*t)},
];
const recipe=await readFile(new URL(import.meta.url));
await publishOriginals(clips.map(c=>{const p=originalPackage('asset.audio.spells.'+c.id,c.name,'audio','authored');p.definition.tags=['spell','audio',c.id];addBytes(p,'audio','wav',wav(c.seconds,c.sample));addBytes(p,'recipe','ts',recipe);return p;}));
