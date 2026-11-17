// SPDX-License-Identifier: 0BSD
import { reject } from './profile.ts';
export function basePath(path: string, field = '', targetOf?: string): void {
  if (!path.length) reject('path_empty', field);
  if (Buffer.byteLength(path) > 4096) reject('byte_limit_exceeded', field);
  if (/[\u0000-\u001f\u007f-\u009f\\]/u.test(path)) reject('path_forbidden_character', field);
  if (path.startsWith('/')) reject('path_absolute', field);
  if (/^[A-Za-z]:/.test(path)) reject('path_drive_prefix', field);
  const parts = path.split('/');
  if (parts.some(part => !part.length)) reject('path_empty_component', field);
  if (targetOf === undefined && parts.some(part => part === '.' || part === '..')) reject('path_dot_component', field);
  if (parts.some(part => Buffer.byteLength(part) > 255)) reject('path_component_too_long', field);
  if (targetOf !== undefined) {
    let depth = targetOf.split('/').length - 1;
    for (const part of parts) {
      if (part === '..') { if (depth === 0) reject('symlink_escape', field); depth--; }
      else if (part !== '.') depth++;
    }
  }
}
export type UnicodeMaps = { upper: number[][]; fold: number[][] };
export function collisionKey(text: string, unicode: UnicodeMaps): string {
  const upper = new Map(unicode.upper.map(pair => [pair[0], pair[1]]));
  const fold = new Map(unicode.fold.map(pair => [pair[0], pair[1]]));
  return Array.from(text, char => {
    const point = char.codePointAt(0)!;
    const raised = upper.get(point) ?? point;
    return String.fromCodePoint(fold.get(raised) ?? raised);
  }).join('');
}
export function windowsComponent(component: string, field = 'Entry.path'): void {
  if (/[<>:"|?*]/u.test(component)) reject('windows_forbidden_character', field);
  if (/[. ]$/.test(component)) reject('windows_trailing_dot_or_space', field);
  const stem = component.split('.')[0].replace(/ +$/, '').replace(/[a-z]/g, char => String.fromCharCode(char.charCodeAt(0) - 32));
  if (/^(?:CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|(?:COM|LPT)[0-9¹²³])$/.test(stem)) reject('windows_reserved_name', field);
}
export function windowsTree(entries: any[], unicode: UnicodeMaps, variant: string): string[] {
  const seen = new Map<string, string>(), warnings: string[] = [];
  for (const entry of entries) {
    const components = entry.path.split('/');
    for (const component of components) windowsComponent(component, 'Entry.path');
    for (let length = 1; length <= components.length; length++) {
      const path = components.slice(0, length).join('/'), key = collisionKey(path, unicode), earlier = seen.get(key);
      if (earlier !== undefined && earlier !== path) reject('windows_case_collision', 'Entry.path');
      seen.set(key, path);
    }
    if (entry.type === 2) {
      if (variant === 'installation') reject('unsupported_symlink', 'Entry.type');
      warnings.push('windows_symlink');
    }
  }
  return warnings;
}
