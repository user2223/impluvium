import { InputError, json } from './common.mjs';
import { fileURLToPath } from 'node:url';
import { toml } from './toml.mjs';

// Admission comes from deny.toml; engine additionally excludes copyleft obligations.
const copyleft = /^(?:MPL|GPL|LGPL|AGPL)(?:-|$)/;
let identifiers;
// Exception names are SPDX vocabulary, not an admission list. No license text
// or parser implementation is included here.
const standardExceptions = new Set((
  '389-exception Asterisk-exception Autoconf-exception-2.0 Autoconf-exception-3.0 Autoconf-exception-generic Autoconf-exception-generic-3.0 Autoconf-exception-macro ' +
  'Bison-exception-1.24 Bison-exception-2.2 Bootloader-exception Classpath-exception-2.0 CLISP-exception-2.0 cryptsetup-OpenSSL-exception DigiRule-FOSS-exception eCos-exception-2.0 ' +
  'Fawkes-Runtime-exception FLTK-exception fmt-exception Font-exception-2.0 freertos-exception-2.0 GCC-exception-2.0 GCC-exception-2.0-note GCC-exception-3.1 Gmsh-exception GNAT-exception ' +
  'GNOME-examples-exception GNU-compiler-exception gnu-javamail-exception GPL-3.0-interface-exception GPL-3.0-linking-exception GPL-3.0-linking-source-exception GPL-CC-1.0 ' +
  'GStreamer-exception-2005 GStreamer-exception-2008 i2p-gpl-java-exception KiCad-libraries-exception LGPL-3.0-linking-exception libpri-OpenH323-exception Libtool-exception Linux-syscall-note ' +
  'LLGPL LLVM-exception LZMA-exception mif-exception OCaml-LGPL-linking-exception OCCT-exception-1.0 OpenJDK-assembly-exception-1.0 openvpn-openssl-exception PS-or-PDF-font-exception-20170817 ' +
  'QPL-1.0-INRIA-2004-exception Qt-GPL-exception-1.0 Qt-LGPL-exception-1.1 Qwt-exception-1.0 SANE-exception SHL-2.0 SHL-2.1 stunnel-exception SWI-exception Swift-exception Texinfo-exception ' +
  'u-boot-exception-2.0 UBDL-exception Universal-FOSS-exception-1.0 vsftpd-openssl-exception WxWindows-exception-3.1 x11vnc-openssl-exception Nokia-Qt-exception-1.1'
).split(' '));
const knownLicense = value => {
  identifiers ??= new Set(['spdx-license-ids.json', 'spdx-deprecated-ids.json'].flatMap(name => json(fileURLToPath(new URL(name, import.meta.url)))));
  return identifiers.has(value) || (value.endsWith('+') && identifiers.has(value.slice(0, -1)));
};

export function policy(file) {
  const allowed = toml(file).data.licenses?.allow;
  if (!Array.isArray(allowed) || !allowed.length || !allowed.every(item => typeof item === 'string')) throw new InputError(`${file}:1: expected nonempty licenses.allow`);
  const permitted = new Set(allowed);
  for (const entry of allowed) {
    const match = /^([A-Za-z0-9][A-Za-z0-9.+-]*)(?: WITH ([A-Za-z0-9][A-Za-z0-9.+-]*))?$/.exec(entry);
    if (!match || !knownLicense(match[1]) || (match[2] && !standardExceptions.has(match[2]))) throw new InputError(`${file}:1: unknown or malformed SPDX allow entry ${entry}`);
  }
  return { permitted };
}

export function parse(expression, rules) {
  if (typeof expression !== 'string' || !expression.trim()) throw new InputError('missing SPDX expression');
  const tokens = expression.match(/[A-Za-z0-9][A-Za-z0-9.+-]*|[()]|\S/g) ?? [];
  let position = 0;
  const fail = message => { throw new InputError(`invalid SPDX expression ${JSON.stringify(expression)}: ${message}`); };
  const atom = () => {
    if (tokens[position] === '(') {
      position++;
      const item = or();
      if (tokens[position++] !== ')') fail('missing closing parenthesis');
      return item;
    }
    const license = tokens[position++];
    if (!license || !knownLicense(license)) fail(`unknown identifier ${license}`);
    let exception;
    if (tokens[position] === 'WITH') {
      position++;
      exception = tokens[position++];
      if (!standardExceptions.has(exception)) fail(`unknown exception ${exception}`);
    }
    return { license, exception };
  };
  const and = () => {
    let item = atom();
    while (tokens[position] === 'AND') { position++; item = { op: 'AND', left: item, right: atom() }; }
    return item;
  };
  const or = () => {
    let item = and();
    // Cargo's deprecated slash separator has exactly the precedence of OR.
    while (tokens[position] === 'OR' || tokens[position] === '/') { position++; item = { op: 'OR', left: item, right: and() }; }
    return item;
  };
  const tree = or();
  if (position !== tokens.length) fail(`unexpected token ${tokens[position]}`);
  return tree;
}

export function acceptable(expression, rules) {
  const evaluate = tree => {
    if (tree.op) {
      const left = evaluate(tree.left), right = evaluate(tree.right);
      return tree.op === 'OR' ? left || right : left && right;
    }
    return !copyleft.test(tree.license) && rules.permitted.has(tree.license + (tree.exception ? ` WITH ${tree.exception}` : ''));
  };
  return evaluate(parse(expression, rules));
}
