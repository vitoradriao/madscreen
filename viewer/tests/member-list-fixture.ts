import { createServers } from '../shared/servers-ui';
import { MadClient } from '../shared/client';
const client = new MadClient('http://127.0.0.1');
const photo = 'data:image/svg+xml,' + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><path fill="#f43f5e" d="M0 0h32v64H0z"/><path fill="#22d3ee" d="M32 0h32v64H32z"/></svg>');
const owner = {id:'owner',username:'owner',displayName:'Ana com um nome de proprietaria muito longo para a lista',avatar:photo,online:true,role:'owner'};
client.setSession({token:'local-fixture',exp:9999999999,user:owner} as any);
const directory = createServers(client, {onWatch(){}});
const shell = document.createElement('div'); shell.className='shell directory-mode unified-shell';
const content = document.createElement('main');content.className='content directory-mode';content.append(directory.root);shell.append(content);document.body.append(shell);
directory.update({me:owner,friends:[],incoming:[],outgoing:[],servers:[{id:'demo',name:'Amigos',ownerId:'owner',role:'owner',members:[owner,{...owner,id:'admin',displayName:'Administrador com outro nome muito longo para a lista de membros',role:'admin'},{...owner,id:'initials',displayName:'Bruno Silva',avatar:null,role:'member'}],channels:[{id:'general',name:'Geral',live:[]}]}]} as any);
directory.setConnected(true);
(window as any).memberReady = true;
(window as any).checkMembers = () => {
 const rows=[...document.querySelectorAll<HTMLElement>('.member-row')];
 if(rows.length!==3)throw Error('Roster ausente');
 if(document.querySelector('.server-members')!.textContent!.includes('(você)'))throw Error('Sufixo indevido');
 return rows.map(row=>{
 const name=row.querySelector<HTMLElement>('.member-name')!,badge=row.querySelector<HTMLElement>('.role'),image=row.querySelector('img'),box=row.getBoundingClientRect(),n=name.getBoundingClientRect(),b=badge?.getBoundingClientRect();
 if(name.parentElement!.children.length!==(badge?2:1))throw Error('DOM nao separa nome/selo');
 if(badge&&(!(name.scrollWidth>name.clientWidth)||b!.right>box.right||b!.left<n.right-1||getComputedStyle(badge).flexShrink!=='0'))throw Error('Nome/selo cortado');
 if(getComputedStyle(name).textOverflow!=='ellipsis')throw Error('Sem reticencias');
 if(image&&getComputedStyle(image).filter!=='none')throw Error('Foto filtrada');
 if(!image&&row.querySelector('.avatar')!.textContent!=='Br')throw Error('Iniciais alteradas');
 return {name:name.textContent,truncated:name.scrollWidth>name.clientWidth,badge:badge?.textContent,filter:image?getComputedStyle(image).filter:null};
 });
};

