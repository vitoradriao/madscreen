import assert from "node:assert/strict";
import { test } from "node:test";
import { createJoinNotice, createJoinSound } from "../../apps/madscreen-app/src/join-sound.ts";
import { diagnosticGuidance } from "../shared/diagnostic-guidance.ts";

test("aviso de entrada ignora hello, duplicatas, prévia e conexão adicional", () => {
  let notices = 0;
  const notice = createJoinNotice("self", () => notices++);
  notice({type:"hello", participants:[{viewerId:"old",uid:"old",name:"Pessoa"}]});
  const join = (viewerId, uid) => notice({type:"viewer_joined",viewerId,uid});
  join("self-preview", "self"); join("old-extra", "old");
  join("new", "friend"); join("new", "friend"); join("new-extra", "friend");
  assert.equal(notices, 1);
  notice({type:"viewer_left",viewerId:"new"}); join("new", "friend");
  assert.equal(notices, 1);
  notice({type:"viewer_left",viewerId:"new"}); notice({type:"viewer_left",viewerId:"new-extra"});
  join("returned", "friend"); assert.equal(notices, 2);
});

const sample = (atMs, bytesReceived, framesDecoded, presentedFrames, extra = {}) =>
  ({atMs,bytesReceived,framesDecoded,presentedFrames,packetsLost:0,segment:1,provider:"livekit",documentHidden:false,...extra});
const base = [sample(1000,100,10,9), sample(2000,200,20,19)];
const guidance = (samples, now=2100, hidden=false) => diagnosticGuidance({samples}, now, hidden).join(" ");
test("observações distinguem recepção, decode e apresentação sem atribuir causa", () => {
  const value = guidance(base);
  assert.match(value, /chegaram dados/); assert.match(value, /novos quadros foram decodificados/);
  assert.match(value, /novos quadros foram apresentados/); assert.match(value, /não o atraso da imagem/);
  const stalled = guidance([base[0],sample(2000,200,10,9,{packetsLost:2})]);
  assert.match(stalled,/contador de perdas aumentou/); assert.match(stalled,/Fonte parada também/);
});
test("dados ausentes, antigos, ocultos, reset e contraditórios ficam indeterminados", () => {
  for (const samples of [[],[base[0]],[base[0],{...base[1],segment:2}],
    [base[0],{...base[1],bytesReceived:0}],[base[0],{...base[1],documentHidden:true}],
    [base[0],{...base[1],atMs:6000}],[base[0],{...base[1],framesDecoded:NaN}],
    [base[0],{...base[1],bytesReceived:100}], [base[0],{...base[1],framesDecoded:10}]]) {
    assert.match(guidance(samples),/Recepção: Não foi possível determinar/);
  }
  assert.match(guidance(base,5000),/Recepção: Não foi possível determinar/);
  assert.match(guidance(base,2100,true),/Recepção: Não foi possível determinar/);
});

test("som agenda duas notas limitadas, respeita preferência e libera contexto", () => {
  const previous = Object.fromEntries(["document","AudioContext","localStorage"].map(key=>[key,globalThis[key]]));
  const notes=[]; let closed=0, disabled=false;
  globalThis.document=new EventTarget();
  globalThis.localStorage={getItem:()=>disabled?"off":null};
  globalThis.AudioContext=class {
    state="running";currentTime=10;destination={};
    createOscillator(){const note={frequency:{value:0},connect(){},disconnect(){},start(at){this.at=at;},stop(at){this.until=at;}};notes.push(note);return note;}
    createGain(){return {gain:{setValueAtTime(){},linearRampToValueAtTime(){},exponentialRampToValueAtTime(){}},connect(){},disconnect(){}};}
    async close(){closed++;}
  };
  try {
    const sound=createJoinSound();
    sound.play(); assert.equal(notes.length,0);
    document.dispatchEvent(new Event("pointerdown"));
    disabled=true;sound.play();assert.equal(notes.length,0);
    disabled=false;sound.play();sound.play();assert.equal(notes.length,2);
    assert.deepEqual(notes.map(note=>note.frequency.value),[660,880]);
    assert.ok(notes.every(note=>note.until-note.at<0.18));
    sound.dispose(); sound.play(); assert.equal(notes.length,2);assert.equal(closed,1);
  } finally { for(const [key,value] of Object.entries(previous)) {if(value===undefined)delete globalThis[key];else globalThis[key]=value;} }
});
