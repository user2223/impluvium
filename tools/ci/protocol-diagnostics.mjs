import { fail } from './protocol-contract.mjs';

// Collect independent input failures; identical records retain all source lines.
export function diagnosticLog() {
  const entries = new Map();
  const add = (file, line, error, raw = '') => {
    const message = error?.message ?? String(error), key = JSON.stringify([file, message, raw]);
    if (!entries.has(key)) entries.set(key, { file, line, lines: [], message, raw });
    const entry = entries.get(key);
    if (!entry.lines.includes(line)) entry.lines.push(line);
  };
  return {
    get errors() { return [...entries.values()]; },
    add,
    attempt(file, line, operation, raw = '') {
      try { return operation(); }
      catch (error) { add(file, line, error, raw); return undefined; }
    },
    async stage(file, operation) {
      try { return await operation(); }
      catch (error) { add(file, 1, error); return undefined; }
    },
    finish() {
      if (entries.size) {
        try { fail([...entries.values()].map(entry => `${entry.file}:${entry.lines.join(',')}: ${entry.message}`).join('\n')); }
        catch (error) { error.diagnosticsCollected = true; throw error; }
      }
    },
  };
}

export function jsonRecords(text, file, report) {
  const records = [];
  for (const [index, raw] of text.split('\n').entries()) {
    if (!raw.trim()) continue;
    const record = report.attempt(file, index + 1, () => {
      const value = JSON.parse(raw);
      if (!value || typeof value !== 'object' || Array.isArray(value)) fail('expected a JSON record object');
      return value;
    }, raw);
    if (record) records.push(record);
  }
  return records;
}
