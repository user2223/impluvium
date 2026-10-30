import { InputError, lineAt, problem } from './common.mjs';

const checkedTools = new Set(['cargo', 'pnpm', 'npm', 'npx', 'corepack', 'yarn']);
const basename = word => word.split('/').at(-1);

// Find a substitution's closing parenthesis without treating quoted ')' as syntax.
function substitution(source, start, file, firstLine) {
  let quote = '', depth = 1;
  for (let i = start + 2; i < source.length; i++) {
    const char = source[i];
    if (char === '\\' && quote !== "'") { i++; continue; }
    if (quote === "'") { if (char === quote) quote = ''; continue; }
    if (char === '$' && source[i + 1] === '(') {
      i = substitution(source, i, file, firstLine).end;
      continue;
    }
    if (quote) { if (char === quote) quote = ''; continue; }
    if (char === "'" || char === '"') quote = char;
    else if (char === '(') depth++;
    else if (char === ')' && --depth === 0) return { end: i, script: source.slice(start + 2, i) };
  }
  throw new InputError(problem(file, firstLine + lineAt(source, start) - 1, 'unterminated shell command substitution'));
}

// Tokenize commands without running a shell. Quotes keep arguments together;
// comments, separators and escaped newlines retain their shell meaning.
export function commands(source, file, firstLine = 1) {
  const result = [];
  const ignored = new Set();
  // Literal shell words, including paths and words within quoted script strings.
  // Keep source offsets so parsing one occurrence cannot excuse another one.
  const occurrences = [...source.matchAll(/[A-Za-z0-9_./@+-]+/g)].flatMap(match => {
    const tool = basename(match[0]);
    const offset = match.index + match[0].length - tool.length;
    return checkedTools.has(tool) ? [{ tool, offset, line: firstLine + lineAt(source, offset) - 1 }] : [];
  });
  const location = offset => firstLine + lineAt(source, offset) - 1;
  function scan(script, offsets) {
    const ownCommands = [];
    let command = [], word = '', positions = [], active = false, literalDollar = false, line = firstLine;
    const append = (char, index, literal = false) => {
      word += char;
      positions.push(offsets[index]);
      if (char === '$' && literal) literalDollar = true;
    };
    const flushWord = () => {
      if (active) command.push({ value: word, line, positions, literalDollar });
      word = ''; positions = []; active = false; literalDollar = false;
    };
    const flush = () => {
      flushWord();
      if (command.length) { result.push(command); ownCommands.push(command); }
      command = [];
    };
    const nested = i => {
      const part = substitution(script, i, file, location(offsets[0]));
      scan(part.script, offsets.slice(i + 2, part.end));
      return part.end;
    };
    for (let i = 0; i < script.length; i++) {
      const char = script[i];
      if (char === '\\' && script[i + 1] === '\n') { i++; continue; }
      if (char === '#' && !active) {
        while (i < script.length && script[i] !== '\n') ignored.add(offsets[i++]);
        flush();
      } else if (/\s/.test(char)) {
        if (char === '\n') flush(); else flushWord();
      } else if (char === '$' && script[i + 1] === '(') {
        if (!active) line = location(offsets[i]);
        active = true;
        const end = nested(i);
        word += '${command-substitution}';
        positions.push(...Array('${command-substitution}'.length).fill(undefined));
        i = end;
      } else if (/[;|&()]/.test(char) || (!active && /[{}]/.test(char) && /[\s;|&}]|^$/.test(script[i + 1] ?? ''))) {
        flush();
      } else if (char === "'" || char === '"') {
        if (!active) line = location(offsets[i]);
        active = true;
        const quote = char;
        let closed = false;
        for (i++; i < script.length; i++) {
          if (script[i] === quote) { closed = true; break; }
          if (quote === '"' && script[i] === '\\' && /["\\$`\n]/.test(script[i + 1] ?? '')) {
            i++; if (script[i] !== '\n') append(script[i], i, true);
          } else if (quote === '"' && script[i] === '$' && script[i + 1] === '(') {
            const end = nested(i);
            word += '${command-substitution}';
            positions.push(...Array('${command-substitution}'.length).fill(undefined));
            i = end;
          } else append(script[i], i, quote === "'");
        }
        if (!closed) throw new InputError(problem(file, line, 'unterminated shell quote'));
      } else if (char === '`') {
        // Keep backtick contents opaque: their tool occurrences remain unverified.
        if (!active) line = location(offsets[i]);
        active = true;
        append(char, i);
        let closed = false;
        for (i++; i < script.length; i++) {
          append(script[i], i);
          if (script[i] === '`') { closed = true; break; }
          if (script[i] === '\\' && i + 1 < script.length) { i++; append(script[i], i); }
        }
        if (!closed) throw new InputError(problem(file, line, 'unterminated shell backtick'));
      } else if (char === '\\') {
        if (!active) line = location(offsets[i]);
        active = true; i++; append(script[i] ?? '', i, true);
      } else {
        if (!active) line = location(offsets[i]);
        active = true; append(char, i);
      }
    }
    flush();
    // Reparse a script argument only when a recognised shell is the executable.
    for (const raw of ownCommands) {
      const tokens = executable(raw);
      if (!/^(?:ba|da|z|k)?sh$/.test(basename(tokens[0]?.value ?? ''))) continue;
      const flag = tokens[1];
      if (/^-[a-z]*c[a-z]*$/.test(flag?.value ?? '') && tokens[2]) {
        scan(tokens[2].value, tokens[2].positions);
      }
    }
  }
  scan(source, Array.from({ length: source.length }, (_, index) => index));
  result.occurrences = occurrences.filter(item => !ignored.has(item.offset));
  return result;
}

export function executable(tokens) {
  let index = 0;
  while (index < tokens.length) {
    const word = tokens[index].value;
    if (/^(?:if|then|elif|else|do|while|until|!|--)$/.test(word) || /^[A-Za-z_][A-Za-z_0-9]*=/.test(word)) { index++; continue; }
    const wrapper = basename(word);
    if (wrapper === 'runuser') {
      if (tokens[index + 1]?.value !== '-u' || !tokens[index + 2] || tokens[index + 2].value.startsWith('-') ||
          tokens[index + 3]?.value !== '--' || !tokens[index + 4]) break;
      index += 4;
      continue;
    }
    if (!['command', 'exec', 'sudo', 'env'].includes(wrapper)) break;
    const start = index;
    index++;
    while (tokens[index]?.value.startsWith('-')) {
      const option = tokens[index++].value;
      if (option === '--') break;
      const argumentsByWrapper = { command: [], exec: ['-a'], sudo: ['-u', '-g', '--user', '--group'],
        env: ['-u', '--unset', '-C', '--chdir'] };
      const flagsByWrapper = { command: /^-p$/, exec: /^-[cl]+$/, sudo: /^-[nHE]+$|^--preserve-env(?:=\w+(?:,\w+)*)?$/,
        env: /^-i$|^--ignore-environment$|^--(?:unset|chdir)=.+$/ };
      if (argumentsByWrapper[wrapper].includes(option) && tokens[index]) index++;
      else if (!flagsByWrapper[wrapper].test(option)) return tokens.slice(start);
    }
  }
  return tokens.slice(index);
}
