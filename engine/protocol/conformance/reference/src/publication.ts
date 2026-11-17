// SPDX-License-Identifier: 0BSD
import { fileURLToPath } from 'node:url';
import { resolve, relative, dirname, isAbsolute, sep } from 'node:path';
import { throws } from 'node:assert';
import { packageDirectory, testdataRelativePath } from './locations.ts';
import { listDeliveredFiles, readDataBytes, readSelfCheckText } from './loader.ts';
const inside=(root:string,path:string):boolean=>{const part=relative(root,path);return !isAbsolute(part)&&(part===''||part!=='..'&&!part.startsWith('..'+sep));};
const fixedLocation=(value:string):boolean=>/^\/(?:home|Users|tmp|etc|usr|var|private|root|mnt|opt|proc|dev)(?:\/|$)/.test(value)||/^[a-z]:[\\/]/i.test(value)||/^file:\/\//i.test(value);
export function checkStaticPaths(code:string,moduleFile:string):void {
 const root=fileURLToPath(packageDirectory);
 for(const match of code.matchAll(/^(?:import|export)\s*(?:type\s+)?(?:[^;]*?\bfrom\s*)?['"]([^'"]+)['"]/gm)){
  const specifier=match[1];if(specifier.startsWith('.')){if(!inside(root,resolve(dirname(moduleFile),specifier)))throw Error('publication_import_escape');}
  else if(!specifier.startsWith('node:')&&specifier!=='@noble/hashes/blake3')throw Error('publication_import_not_allowed');
 }
 for(const match of code.matchAll(/(['"])((?:\\.|[^'"\n])*)\1/g)){
  const value=match[2];if(fixedLocation(value))throw Error('publication_fixed_location');
  if(value.startsWith('../')||value.startsWith('./')){if(moduleFile===fileURLToPath(new URL('src/locations.ts',packageDirectory))&&value===testdataRelativePath)continue;if(!inside(root,resolve(dirname(moduleFile),value)))throw Error('publication_relative_path_escape');}
 }
}
export async function publicationGate():Promise<{id:string;result:'pass';path:string;evidence:unknown}>{
 const files=await listDeliveredFiles();
 for(const file of files){
  // Package metadata and licence files need only their size and entry type here.
  if(file.root==='package'&&!/\.(?:ts|mjs)$/.test(file.file))continue;
  const text=file.root==='package'?await readSelfCheckText(file.file):(await readDataBytes(file.file)).toString('utf8');
  if(file.root==='package'&&/\.(?:ts|mjs)$/.test(file.file))checkStaticPaths(text,fileURLToPath(new URL(file.file,packageDirectory)));
  if(file.file.endsWith('.json')){const visit=(value:unknown):void=>{if(value&&typeof value==='object')for(const[key,item]of Object.entries(value)){if(['source','document','file','directory'].includes(key)&&typeof item==='string'&&(fixedLocation(item)||item.startsWith('../')))throw Error('publication_metadata_location');visit(item);}};visit(JSON.parse(text));}
 }
 const probe=fileURLToPath(new URL('src/probe.ts',packageDirectory));
 throws(()=>checkStaticPaths('import "'+['..','..','..','external.ts'].join('/')+'";',probe),/publication_import_escape/);
 throws(()=>checkStaticPaths('const path="'+'/'+['home','example','file'].join('/')+'";',probe),/publication_fixed_location/);
 throws(()=>checkStaticPaths('const path="'+['..','..','..','external/'].join('/')+'";',probe),/publication_relative_path_escape/);
 return{id:'publication.paths',result:'pass',path:'src/publication.ts',evidence:{files:files.length,negativePathProbes:3}};
}
