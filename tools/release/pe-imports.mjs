// Read PE imports without executing the candidate or requiring a platform SDK.
import { readFile, readdir, writeFile, access } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function peImports(bytes) {
  const buffer=Buffer.from(bytes);
  if(buffer.length<256||buffer.toString('ascii',0,2)!=='MZ')throw Error('PE invalido');
  const pe=buffer.readUInt32LE(0x3c);
  if(pe+24>buffer.length||buffer.toString('binary',pe,pe+4)!=='PE\0\0')throw Error('Cabecalho PE invalido');
  const count=buffer.readUInt16LE(pe+6),optionalSize=buffer.readUInt16LE(pe+20),optional=pe+24;
  if(count>96||optional+optionalSize+40*count>buffer.length||buffer.readUInt16LE(optional)!==0x20b)throw Error('Esperado PE32+ limitado');
  const sectionTable=optional+optionalSize;
  const offset=rva=>{
    for(let index=0;index<count;index++){
      const section=sectionTable+40*index,base=buffer.readUInt32LE(section+12),size=buffer.readUInt32LE(section+16),raw=buffer.readUInt32LE(section+20);
      if(rva>=base&&rva-base<size&&raw+rva-base<buffer.length)return raw+rva-base;
    }
    throw Error('RVA fora do arquivo');
  };
  const string=rva=>{const start=offset(rva),end=buffer.indexOf(0,start);if(end<start||end-start>256)throw Error('Nome PE invalido');const name=buffer.toString('ascii',start,end);if(!/^[a-z0-9_.-]+\.dll$/i.test(name))throw Error('Importacao PE invalida');return name.toLowerCase();};
  if(optionalSize<112+14*8)throw Error('Diretorios PE ausentes');
  const imports=[];
  for(const [directory,stride,nameOffset,kind] of [[1,20,12,'import'],[13,32,4,'delay-import']]){
    const rva=buffer.readUInt32LE(optional+112+directory*8),size=buffer.readUInt32LE(optional+116+directory*8);
    if(!rva)continue;
    const start=offset(rva);let terminated=false;
    for(let index=0;index<Math.min(4096,Math.floor(size/stride)+1);index++){
      const at=start+index*stride;if(at+stride>buffer.length)throw Error('Diretorio PE truncado');
      if(buffer.subarray(at,at+stride).every(value=>value===0)){terminated=true;break;}
      if(kind==='delay-import'&&(buffer.readUInt32LE(at)&1)===0)throw Error('Delay import VA nao suportado');
      imports.push({name:string(buffer.readUInt32LE(at+nameOffset)),kind});
    }
    if(!terminated)throw Error('Diretorio PE sem terminador');
  }
  return imports;
}

if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
  try{
    const [input,output,...extra]=process.argv.slice(2);if(!input||!output||extra.length)throw Error('Uso: node tools/release/pe-imports.mjs pasta-bin arquivo-novo.json');
    const directory=resolve(input),files=(await readdir(directory)).filter(name=>/\.dll$/i.test(name)),provided=new Set(files.map(name=>name.toLowerCase()));
    const reports=[];
    for(const file of files){const imports=[];for(const item of peImports(await readFile(join(directory,file)))){
      let resolution=provided.has(item.name)?'provided':'unresolved';
      if(resolution==='unresolved'){try{await access(join(process.env.WINDIR??'C:/Windows','System32',item.name));resolution='present-on-current-Windows';}catch{if(/^api-ms-win-|^ext-ms-win-/.test(item.name))resolution='api-set-contract';}}
      imports.push({...item,resolution});
    }reports.push({file,imports});}
    await writeFile(output,JSON.stringify({schema:1,files:reports,limitations:'PE imports only, not dynamically loaded dependencies, older Windows qualification, licenses or legal approval.'},null,2)+'\n',{flag:'wx'});
    console.log(JSON.stringify({files:reports.length,unresolved:reports.flatMap(row=>row.imports).filter(row=>row.resolution==='unresolved').length}));
  }catch(error){console.error(error.code?'Falha ao ler/gravar inventario PE.':error.message);process.exitCode=1;}
}
