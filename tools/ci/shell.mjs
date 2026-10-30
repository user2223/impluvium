import { InputError, lineAt, problem } from './common.mjs';

// Tokenize commands without running a shell. Quotes keep arguments together;
// comments, separators and escaped newlines retain their shell meaning.
export function commands(source, file, firstLine = 1) {
  const result = [];
  let command = [], word = '', active = false, line = firstLine;
  const flushWord = () => {
    if (active) command.push({ value: word, line });
    word = ''; active = false;
  };
  const flush = () => { flushWord(); if (command.length) result.push(command); command = []; };
  for (let i = 0; i < source.length; i++) {
    const char = source[i];
    if (char === '\\' && source[i + 1] === '\n') { i++; continue; }
    if (char === '#' && !active) {
      while (i < source.length && source[i] !== '\n') i++;
      flush();
    } else if (/\s/.test(char)) {
      if (char === '\n') flush(); else flushWord();
    } else if (/[;|&()]/.test(char)) {
      flush();
    } else if (char === "'" || char === '"') {
      if (!active) line = firstLine + lineAt(source, i) - 1;
      active = true;
      const quote = char;
      let closed = false;
      for (i++; i < source.length; i++) {
        if (source[i] === quote) { closed = true; break; }
        if (quote === '"' && source[i] === '\\' && /["\\$`\n]/.test(source[i + 1] ?? '')) {
          i++; if (source[i] !== '\n') word += source[i];
        } else word += source[i];
      }
      if (!closed) throw new InputError(problem(file, line, 'unterminated shell quote'));
    } else if (char === '\\') {
      if (!active) line = firstLine + lineAt(source, i) - 1;
      active = true; word += source[++i] ?? '';
    } else {
      if (!active) line = firstLine + lineAt(source, i) - 1;
      active = true; word += char;
    }
  }
  flush();
  // Inspect scripts passed explicitly to a shell, while leaving echo/printf
  // arguments alone. Here-document bodies are already tokenized as commands.
  for (const command of [...result]) {
    const start = command.findIndex(token => /^(?:ba|da|z|k)?sh$/.test(token.value));
    if (start >= 0) {
      const flag = command.findIndex((token, i) => i > start && /^-[a-z]*c[a-z]*$/.test(token.value));
      if (flag >= 0 && command[flag + 1]) {
        result.push(...commands(command[flag + 1].value, file, command[flag + 1].line));
      }
    }
  }
  return result;
}

export function executable(tokens) {
  let index = 0;
  while (index < tokens.length) {
    const word = tokens[index].value;
    if (/^(?:if|then|elif|do|while|until|!|--)$/.test(word) || /^[A-Za-z_][A-Za-z_0-9]*=/.test(word)) { index++; continue; }
    const wrapper = word.split('/').at(-1);
    if (!['command', 'exec', 'sudo', 'env'].includes(wrapper)) break;
    index++;
    while (tokens[index]?.value.startsWith('-')) {
      const option = tokens[index++].value;
      if (/^(?:-u|-g|--user|--group|--unset|-C|--chdir)$/.test(option)) index++;
      if (option === '--') break;
    }
  }
  return tokens.slice(index);
}
