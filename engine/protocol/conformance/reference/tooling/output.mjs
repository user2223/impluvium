// SPDX-License-Identifier: 0BSD
import { mkdir, realpath, lstat, stat, open, rename, unlink } from 'node:fs/promises';
import { O_CREAT, O_EXCL, O_WRONLY, O_NOFOLLOW } from 'node:constants';
import { randomBytes } from 'node:crypto';
import { resolve, relative, isAbsolute, sep, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { packageDirectory, testdataDirectory } from '../src/locations.ts';
const inside=(root,path)=>{const part=relative(root,path);return !isAbsolute(part)&&(part===''||part!=='..'&&!part.startsWith('..'+sep));};
async function existingPath(path) {
 try{return await realpath(path);}catch(error){if(error.code!=='ENOENT')throw error;const parent=dirname(path);if(parent===path)throw error;return join(await existingPath(parent),path.slice(parent.length+(parent.endsWith(sep)?0:1)));}
}
export async function writeReport(out,name,report) {
 if(typeof out!=='string'||!out)throw Error('Required output directory');
 if(typeof name!=='string'||!/^[a-z][a-z0-9-]*$/.test(name))throw Error('Invalid report name');
 const requested=resolve(out),roots=await Promise.all([packageDirectory,testdataDirectory].map(url=>realpath(fileURLToPath(url))));
 const before=await existingPath(requested);if(roots.some(root=>inside(root,before)))throw Error('output_source_containment');
 await mkdir(requested,{recursive:true});const actual=await realpath(requested);
 if(roots.some(root=>inside(root,actual)))throw Error('output_source_containment');
 const serialized=JSON.stringify(report);if(serialized===undefined)throw Error('Invalid report');
 const path=join(actual,name+'.json');let temporary,handle;
 try{
  for(let attempt=0;attempt<8;attempt++){
   if(await realpath(actual)!==actual)throw Error('output_path_changed');
   const candidate=join(actual,'.'+name+'.'+randomBytes(16).toString('hex')+'.tmp');
   try{handle=await open(candidate,O_CREAT|O_EXCL|O_WRONLY|O_NOFOLLOW,0o600);temporary=candidate;break;}catch(error){if(error.code!=='EEXIST')throw error;}
  }
  if(!handle)throw Error('output_temporary_collision_limit');
  if(await realpath(dirname(temporary))!==actual)throw Error('output_path_changed');
  await handle.writeFile(serialized+'\n');await handle.sync();await handle.close();handle=undefined;
  if(await realpath(dirname(path))!==actual)throw Error('output_path_changed');
  try{
   const info=await lstat(path);
   if(info.isDirectory())throw Error('output_target_directory');
   if(info.isSymbolicLink()){
    try{if((await stat(path)).isDirectory())throw Error('output_target_directory');}catch(error){if(error.code!=='ENOENT')throw error;}
   }else if(!info.isFile())throw Error('output_not_regular_file');
  }catch(error){if(error.code!=='ENOENT')throw error;}
  await rename(temporary,path);temporary=undefined;
 }finally{
  try{if(handle)await handle.close();}finally{if(temporary)try{await unlink(temporary);}catch(error){if(error.code!=='ENOENT')throw error;}}
 }
}
