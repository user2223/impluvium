// SPDX-License-Identifier: 0BSD
import { defaults, enumValue, reject, timestampRange, utf8 } from './profile.ts';
import type { Schema } from './profile.ts';

export function integerText(text: string, minimum: bigint, maximum: bigint, field = ''): bigint {
  const match = /^(-?)(0|[1-9][0-9]*)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?$/.exec(text);
  if (!match) reject('invalid_number', field);
  let digits = (match[2] + (match[3] ?? '')).replace(/^0+/, '');
  if (!digits) return 0n;
  let scale = BigInt(match[4] ?? '0') - BigInt((match[3] ?? '').length);
  if (scale < 0n) {
    const trailing = digits.length - digits.replace(/0+$/, '').length;
    if (-scale > BigInt(trailing)) reject('invalid_number', field);
    digits = digits.slice(0, digits.length - Number(-scale)); scale = 0n;
  }
  const boundLength = (maximum < -minimum ? -minimum : maximum).toString().length;
  if (BigInt(digits.length) + scale > BigInt(boundLength)) reject('integer_out_of_range', field);
  const value = BigInt((match[1] ? '-' : '') + digits + '0'.repeat(Number(scale)));
  if (value < minimum || value > maximum) reject('integer_out_of_range', field);
  return value;
}
export function base64Bytes(text: string, field = '', strictUrl = false): Buffer {
  if (strictUrl) {
    if (!/^[A-Za-z0-9_-]*$/.test(text) || text.length % 4 === 1) reject('invalid_base64', field);
    const bytes = Buffer.from(text, 'base64url');
    if (bytes.toString('base64url') !== text) reject('invalid_base64', field);
    return bytes;
  }
  if (!/^[A-Za-z0-9+/_-]*={0,2}$/.test(text) || (/[+/]/.test(text) && /[-_]/.test(text))) reject('invalid_base64', field);
  const bare = text.replace(/=+$/, ''), padding = text.length - bare.length;
  if (bare.length % 4 === 1 || (padding && (text.length % 4 !== 0 || padding !== (4 - bare.length % 4) % 4))) reject('invalid_base64', field);
  return Buffer.from(text, 'base64');
}
export function jsonTimestamp(text: string, field = ''): Record<string, any> {
  const match = /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?([Zz]|([+-])(\d{2}):(\d{2}))$/.exec(text);
  if (!match) reject('invalid_timestamp', field);
  const year = Number(match[1]), month = Number(match[2]), day = Number(match[3]);
  const hour = Number(match[4]), minute = Number(match[5]), second = Number(match[6]);
  const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const months = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (year < 1 || month < 1 || month > 12 || day < 1 || day > months[month - 1] || hour > 23 || minute > 59 || second > 59 || Number(match[10] ?? 0) > 23 || Number(match[11] ?? 0) > 59) reject('invalid_timestamp', field);
  const before = (year: number) => (year - 1) * 365 + Math.floor((year - 1) / 4) - Math.floor((year - 1) / 100) + Math.floor((year - 1) / 400);
  const days = before(year) - before(1970) + months.slice(0, month - 1).reduce((a, b) => a + b, 0) + day - 1;
  const offset = (Number(match[10] ?? 0) * 60 + Number(match[11] ?? 0)) * 60 * (match[9] === '-' ? -1 : 1);
  const model = { seconds: String(days * 86400 + hour * 3600 + minute * 60 + second - offset), nanos: Number((match[7] ?? '').padEnd(9, '0')) };
  timestampRange(model, field); return model;
}
type JsonObjectPolicy = {required?:Record<string,string[]>;nonNull?:Record<string,string[]>};
export function scanJson(bytes: Uint8Array, message: string, schema: Schema, limit = 65536, collect = true, policy:JsonObjectPolicy={}): Record<string, any> {
  if (bytes.length > limit) reject('body_too_large');
  const text = utf8(bytes, 'invalid_json');
  if (text.charCodeAt(0) === 0xfeff) reject('invalid_json');
  let at = 0;
  let chunkIndexCount = 0;
  const ws = () => { while (/[\x20\t\r\n]/.test(text[at] ?? '\0')) at++; };
  const string = (maximum?: number, field = '', beforeLimits?: () => void, capture = true, overflow?:{exceeded:boolean}): string => {
    if (text[at++] !== '"') reject('invalid_json', field);
    const pieces: string[] = [];
    let length = 0;
    while (at < text.length) {
      let char = text[at++];
      if (char === '"') return pieces.join('');
      if (char.charCodeAt(0) < 32) reject('invalid_json', field);
      if (char === '\\') {
        const escape = text[at++];
        const simple: Record<string, string> = { '"': '"', '\\': '\\', '/': '/', b: '\b', f: '\f', n: '\n', r: '\r', t: '\t' };
        if (escape === 'u') {
          const code = text.slice(at, at + 4);
          if (!/^[0-9a-fA-F]{4}$/.test(code)) reject('invalid_json', field);
          at += 4;
          let point = parseInt(code, 16);
          if (point >= 0xd800 && point <= 0xdbff) {
            if (text.slice(at, at + 2) !== '\\u') reject('invalid_json', field);
            const low = text.slice(at + 2, at + 6);
            if (!/^[0-9a-fA-F]{4}$/.test(low) || parseInt(low, 16) < 0xdc00 || parseInt(low, 16) > 0xdfff) reject('invalid_json', field);
            point = 0x10000 + (point - 0xd800) * 1024 + parseInt(low, 16) - 0xdc00; at += 6;
          } else if (point >= 0xdc00 && point <= 0xdfff) reject('invalid_json', field);
          char = String.fromCodePoint(point);
        } else {
          if (!(escape in simple)) reject('invalid_json', field);
          char = simple[escape];
        }
      } else if (char.charCodeAt(0) >= 0xd800 && char.charCodeAt(0) <= 0xdbff) char += text[at++];
      length += Buffer.byteLength(char, 'utf8');
      if (maximum !== undefined && length > maximum) {
        if(overflow)overflow.exceeded=true;
        else{beforeLimits?.();reject('byte_limit_exceeded', field);}
      }
      if(capture&&!overflow?.exceeded)pieces.push(char);
    }
    return reject('invalid_json', field);
  };
  let syntaxAt = -1;
  function syntaxToken(field:string):void {
    ws();if(at===syntaxAt)return;const start=at,first=text[at];
    if(first==='"')string(undefined,field,undefined,false);
    else if(first==='t'||first==='f'||first==='n'){
      const literal=first==='t'?'true':first==='f'?'false':'null';
      if(text.slice(at,at+literal.length)!==literal)reject('invalid_json',field);at+=literal.length;
      if(at<text.length&&!/[\x20\t\r\n,}\]]/.test(text[at]))reject('invalid_json',field);
    }else if(first==='-'||/[0-9]/.test(first??'')){
      const token=/^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(text.slice(at));
      if(!token)reject('invalid_json',field);at+=token[0].length;
      if(at<text.length&&!/[\x20\t\r\n,}\]]/.test(text[at]))reject('invalid_json',field);
    }else if(first!=='{'&&first!=='[')reject('invalid_json',field);
    at=start;syntaxAt=start;
  }
  function value(field: any, parent: string, depth: number, fieldPath: string, element = false, beforeLimits?: () => void): any {
    ws();syntaxToken(fieldPath); const first = text[at];
    if (first === 't' && text.slice(at, at + 4) !== 'true' || first === 'f' && text.slice(at, at + 5) !== 'false' || first === 'n' && text.slice(at, at + 4) !== 'null') reject('invalid_json', fieldPath);
    if (!['"', '{', '[', 't', 'f', 'n', '-'].includes(first) && !/[0-9]/.test(first ?? '')) reject('invalid_json', fieldPath);
    if (text.slice(at, at + 4) === 'null') { at += 4; if (field&&(element||policy.nonNull?.[parent]?.includes(field.name))) reject('json_type_mismatch', fieldPath); return undefined; }
    const kind = first === '"' ? 'string' : first === '{' ? 'object' : first === '[' ? 'array' : first === 't' || first === 'f' ? 'bool' : 'number';
    if (field) {
      let permitted: string[];
      if (field.repeated && !element) permitted = [field.map ? 'object' : 'array'];
      else if (field.type === 'string' || field.type === 'bytes' || field.type === 'Timestamp') permitted = ['string'];
      else if (field.type === 'bool') permitted = ['bool'];
      else if (['uint32', 'uint64'].includes(field.type) || schema.enums[field.type]) permitted = ['number', 'string'];
      else permitted = ['object'];
      if (!permitted.includes(kind)) reject('json_type_mismatch', fieldPath);
    }
    if(kind!=='object'&&kind!=='array'){beforeLimits?.();beforeLimits=undefined;}
    if (kind === 'object' || kind === 'array') {
      if (depth + 1 > 64) reject('nesting_too_deep', fieldPath);
      beforeLimits?.();
      at++;
      if (kind === 'array') {
        const array: any[] = []; let count = 0; ws();
        if (text[at] !== ']') while (true) {
          // Count complete uint32 zero tokens without retaining a decoded array in a precheck.
          if(!collect&&field?.type==='uint32'){
            const shared=parent==='Entry'&&field.name==='chunk_indexes',remaining=field.maxElements-(shared?chunkIndexCount:count);
            const start=at;while((at-start)/2<=remaining&&text.charCodeAt(at)===48&&text.charCodeAt(at+1)===44)at+=2;
            const run=(at-start)/2;
            if(run){if(shared)chunkIndexCount+=run;else count+=run;if((shared?chunkIndexCount:count)>field.maxElements)reject('element_limit_exceeded',shared?'':fieldPath);ws();}
          }
          const item = value(field, parent, depth + 1, fieldPath, true, () => {
            if(field){const shared=parent==='Entry'&&field.name==='chunk_indexes';const next=shared?++chunkIndexCount:++count;if(next>field.maxElements)reject('element_limit_exceeded',shared?'':fieldPath);}
          });
          if (collect && field && item !== undefined) array.push(item);
          ws(); if (text[at] !== ',') break; at++; ws();
        }
        if (text[at++] !== ']') reject('invalid_json', fieldPath);
        return field ? array : undefined;
      }
      if (field?.map) {
        const array: any[] = [], keys = new Set<string>(); let count = 0; ws();
        if (text[at] !== '}') while (true) {
          const overflow={exceeded:false},key=string(field.keyMaxBytes,fieldPath,undefined,true,overflow); ws(); if (text[at++] !== ':') reject('invalid_json', fieldPath);
          // Earlier accepted keys fit the bound, so an overlong key cannot equal one of them.
          if(!overflow.exceeded){if(keys.has(key))reject('duplicate_json_key');keys.add(key);}
          const item = value({ type: 'string', maxBytes: field.valueMaxBytes }, parent, depth + 1, fieldPath, true, () => {
            if (++count > field.maxElements) reject('element_limit_exceeded', fieldPath);
            if (overflow.exceeded) reject('byte_limit_exceeded', fieldPath);
          });
          if(collect)array.push({ key, value: item });
          ws(); if (text[at] !== ',') break; at++; ws();
        }
        if (text[at++] !== '}') reject('invalid_json', fieldPath); return array;
      }
      return object(field ? field.type : undefined, depth + 1);
    }
    let raw: string | boolean;
    if (kind === 'string') raw = string(field?.type === 'string' ? field.maxBytes : field?.type === 'bytes' && field.maxBytes !== undefined ? Math.ceil((field.maxBytes+1)*4/3)+4 : undefined, fieldPath, beforeLimits, !!field);
    else if (kind === 'bool') {
      const literal = text.slice(at, at + (first === 't' ? 4 : 5));
      if (literal !== 'true' && literal !== 'false') reject('invalid_json', fieldPath);
      raw = literal === 'true'; at += literal.length;
      if (at < text.length && !/[\x20\t\r\n,}\]]/.test(text[at])) reject('invalid_json', fieldPath);
    } else {
      const match = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(text.slice(at));
      if (!match) reject('invalid_json', fieldPath); raw = match[0]; at += raw.length;
      if (at < text.length && !/[\x20\t\r\n,}\]]/.test(text[at])) reject('invalid_json', fieldPath);
    }
    beforeLimits?.();
    if (!field) return undefined;
    if (field.type === 'string' && field.maxBytes !== undefined && Buffer.byteLength(raw as string) > field.maxBytes) reject('byte_limit_exceeded', fieldPath);
    if (field.type === 'string' || field.type === 'bool') return raw;
    if (field.type === 'bytes') {
      const encoded = raw as string, bareLength = encoded.replace(/=+$/, '').length;
      if (field.maxBytes !== undefined && Math.floor(bareLength * 3 / 4) > field.maxBytes) reject('byte_limit_exceeded', fieldPath);
      return base64Bytes(encoded, fieldPath).toString('hex');
    }
    if (field.type === 'Timestamp') return jsonTimestamp(raw as string, fieldPath);
    if (schema.enums[field.type]) {
      if (kind === 'string') return enumValue(raw as string, field, parent, schema);
      return enumValue(Number(integerText(raw as string, -2147483648n, 2147483647n, fieldPath)), field, parent, schema);
    }
    const integer = integerText(raw as string, 0n, field.type === 'uint32' ? 4294967295n : 18446744073709551615n, fieldPath);
    return field.type === 'uint32' ? Number(integer) : integer.toString();
  }
  function object(name: string | undefined, depth: number): Record<string, any> {
    const model = name ? defaults(name, schema) : {}, names = new Set<string>(), fields = new Set<string>(), oneofs = new Set<string>();
    ws();
    if (text[at] !== '}') while (true) {
      const key = string(); ws(); if (text[at++] !== ':') reject('invalid_json'); ws();
      const field = name ? schema.messages[name].fields.find(field => field.name === key || field.name.replace(/_([a-z])/g, (_: string, c: string) => c.toUpperCase()) === key) : undefined;
      if (names.has(key)) reject('duplicate_json_key'); names.add(key);
      if (field) {
        const path = name + '.' + field.name;
        if (field.oneof ? oneofs.has(field.oneof) : fields.has(field.name)) reject(field.oneof ? 'oneof_conflict' : 'duplicate_field', field.oneof ? '' : path);
        fields.add(field.name); if (field.oneof) oneofs.add(field.oneof);
      }
      const item = value(field, name ?? '', depth, field ? name + '.' + field.name : '');
      if (field && item !== undefined) model[field.name] = item;
      ws(); if (text[at] !== ',') break; at++; ws();
    }
    if (text[at++] !== '}') reject('invalid_json');
    if(name)for(const field of policy.required?.[name]??[])if(!fields.has(field))reject('missing_field');
    return model;
  }
  ws(); if (text[at++] !== '{') reject('invalid_json');
  const model = object(message, 1); ws(); if (at !== text.length) reject('invalid_json');
  return model;
}
