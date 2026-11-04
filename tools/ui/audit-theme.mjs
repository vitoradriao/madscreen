import assert from "node:assert/strict";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root=fileURLToPath(new URL("../../",import.meta.url));
const css=await readFile(path.join(root,"viewer/shared/tokens.css"),"utf8");
const design=JSON.parse(await readFile(path.join(root,"docs/design/tokens.json"),"utf8"));
const luminance=hex=>{
  const rgb=[1,3,5].map(at=>parseInt(hex.slice(at,at+2),16)/255).map(value=>value<=0.04045?value/12.92:((value+0.055)/1.055)**2.4);
  return 0.2126*rgb[0]+0.7152*rgb[1]+0.0722*rgb[2];
};
const contrast=(left,right)=>{const a=luminance(left),b=luminance(right);return (Math.max(a,b)+0.05)/(Math.min(a,b)+0.05);};
const rows=[];
for (const theme of design.color.themes) {
  const tokens=Object.fromEntries(design.color.tokens.map(token=>[token.name,token.value[theme.id]]));
  for(const background of ["bg","bg-sidebar","surface-1","surface-2","surface-3","surface-4"]){
    for(const foreground of ["text","text-2","text-3","border-strong","focus"]){
      const minimum=foreground==="border-strong"||foreground==="focus"?3:theme.id==='contrast'?7:4.5;
      const ratio=contrast(tokens[foreground],tokens[background]);
      rows.push({theme:theme.id,foreground,background,ratio:Number(ratio.toFixed(3)),minimum});
      assert(ratio>=minimum,`Contraste insuficiente: ${theme.id} ${foreground}/${background} = ${ratio}`);
    }
  }
  for(const background of ["accent","accent-hover"]){
    const ratio=contrast(tokens["accent-fg"],tokens[background]);
    rows.push({theme:theme.id,foreground:"accent-fg",background,ratio:Number(ratio.toFixed(3)),minimum:4.5});assert(ratio>=4.5);
  }
}
const sources=[
  ["viewer/shared/tokens.css",css.replace(/:root\s*\{[\s\S]*?\n\}/,"")],
  ...await Promise.all(["apps/madscreen-app/src/styles.css","apps/madscreen-app/src/views/picker.css","viewer/shared/redesign.css","viewer/shared/design-components.css"].map(async name=>[name,await readFile(path.join(root,name),"utf8")])),
];
const worker=await readFile(path.join(root,"tools/cloudflare/worker/src/index.ts"),"utf8");
sources.push(["Worker notice",worker.match(/<style>([\s\S]*?)<\/style>/)?.[1]??""]);
for(const [name,source]of sources)assert(!/#[0-9a-f]{3,8}\b|\brgba?\(/i.test(source),`Cor fora dos tokens: ${name}`);
const result={ok:true,checks:rows.length,sources:sources.map(([name])=>name),rows};
const output=path.join(root,"bench-results/ux-2026-10-31");await mkdir(output,{recursive:true});
await writeFile(path.join(output,"contrast.json"),JSON.stringify(result,null,2)+"\n");
console.log(JSON.stringify({ok:result.ok,themes:3,contrastPairs:rows.length,hardcodedColorsOutsideTokens:0,minimumText:Math.min(...rows.filter(row=>row.minimum>=4.5).map(row=>row.ratio)),minimumControls:Math.min(...rows.filter(row=>row.minimum===3).map(row=>row.ratio))},null,2));
