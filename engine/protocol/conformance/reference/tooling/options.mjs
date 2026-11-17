// SPDX-License-Identifier: 0BSD
import {resolve,relative,isAbsolute} from 'node:path';
import {fileURLToPath} from 'node:url';
import {testdataDirectory,packageDirectory} from '../src/locations.ts';
export function parseOptions(args, mode) {
  const options={};
  for(let at=0;at<args.length;at+=2){
    const key=args[at],value=args[at+1];
    if(!['--out','--stage'].includes(key)||!value||value.startsWith('--')||Object.hasOwn(options,key))throw Error('Invalid or duplicate option');
    options[key]=value;
  }
  if(!options['--out'])throw Error('Required option: --out <directory>');
  if(mode==='stage'&&!/^[3456]$/.test(options['--stage']??''))throw Error('Required option: --stage 3, 4, 5 or 6');
  if(mode==='vectors'&&options['--stage']!==undefined&&options['--stage']!=='4')throw Error('Vector selection requires --stage 4');
  if(!['stage','vectors'].includes(mode)&&options['--stage']!==undefined)throw Error('Unexpected stage option');
  const out=resolve(options['--out']);
  for(const url of [testdataDirectory,packageDirectory]){
    const path=relative(fileURLToPath(url),out);
    if(!path||!isAbsolute(path)&&path!=='..'&&!path.startsWith('..'+(process.platform==='win32'?'\\':'/')))throw Error('Output must be outside the source and test data trees');
  }
  return {out,stage:options['--stage']===undefined?undefined:Number(options['--stage'])};
}
