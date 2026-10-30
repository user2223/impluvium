import path from 'node:path';
import { existsSync } from 'node:fs';
import { cli, files, InputError, json, keyLine, lineAt, problem, read, stringOption } from './common.mjs';
import { dependencies, manifests } from './cargo.mjs';
import { commands, executable, hasToolWord, withoutRedirections } from './shell.mjs';
import { toml } from './toml.mjs';
import { walkYaml, yaml } from './yaml.mjs';

const requiredJobs = ['checks', 'rust', 'server', 'desktop-ui', 'server-image', 'client-linux'];
const nonResolvingCargo = new Set(['fmt', '--version', '-V', 'help', 'version']);
const toolPin = /^(?:[A-Z][A-Z_0-9]*_(?:VERSION|SHA256)|RUST_NIGHTLY|RUSTUP_TOOLCHAIN)$/;
const packageActions = {
  pnpm: { install: ['install', 'i'], allowed: ['run', 'exec', 'help', 'list', 'ls', 'why', '--version', '-v', '--help', '-h'], download: ['dlx', 'create'] },
  npm: { install: ['install', 'i', 'ci'], allowed: ['run', 'run-script', 'help', 'list', 'ls', 'why', '--version', '-v', '--help', '-h'], download: ['exec', 'x', 'create', 'init'] },
  yarn: { install: ['install'], allowed: ['run', 'exec', 'help', 'list', 'why', '--version', '-v', '--help', '-h'], download: ['dlx', 'create'] },
};
const lockChangingActions = new Set(['update', 'upgrade', 'add', 'remove', 'up', 'dedupe', 'import',
  'uninstall', 'un', 'unlink', 'link', 'rm', 'upgrade-interactive', 'rebuild', 'patch', 'patch-commit', 'set', 'config']);

function imageParts(image) {
  const withoutDigest = image.split('@')[0];
  const slash = withoutDigest.lastIndexOf('/');
  const colon = withoutDigest.lastIndexOf(':');
  return { name: colon > slash ? withoutDigest.slice(0, colon) : withoutDigest,
    tag: colon > slash ? withoutDigest.slice(colon + 1) : '' };
}

function packageAction(tokens) {
  let index = 1;
  if (['--version', '-v', '--help', '-h'].includes(tokens[index]?.value)) {
    return { index, action: tokens[index].value };
  }
  while (tokens[index]?.value.startsWith('-')) {
    const option = tokens[index++].value;
    if (['--filter', '-F', '--dir', '-C', '--prefix'].includes(option)) {
      if (!tokens[index]) return undefined;
      index++;
    } else if (!/^(?:--(?:filter|dir|prefix)=.+|-r|--recursive|-w|--workspace-root|--silent)$/.test(option)) return undefined;
  }
  return { index, action: tokens[index]?.value };
}

// A package name in a lock does not prove which binary an exec launcher will
// select. Reject absent packages; leave that remaining ambiguity as exit 2.
function downloadIsLocked(root, spec) {
  const match = /^(@[^/@\s]+\/[^/@\s]+|[^/@\s]+)(?:@[^\s]+)?$/.exec(spec ?? '');
  if (!match) return undefined;
  const name = match[1];
  try {
    for (const lock of ['pnpm-lock.yaml', 'package-lock.json', 'npm-shrinkwrap.json']) {
      const file = path.join(root, lock);
      if (!existsSync(file)) continue;
      if (lock === 'pnpm-lock.yaml') {
        const packages = yaml(file).value?.packages?.value;
        if (!packages || typeof packages !== 'object' || Array.isArray(packages)) return undefined;
        return Object.keys(packages).some(key => key.startsWith(name + '@'));
      }
      const data = json(file);
      if (!data.packages || typeof data.packages !== 'object' || Array.isArray(data.packages)) return undefined;
      return Object.keys(data.packages).some(key => key === `node_modules/${name}` || key.endsWith(`/node_modules/${name}`));
    }
  } catch (error) {
    if (error instanceof InputError) return undefined;
    throw error;
  }
  if (existsSync(path.join(root, 'yarn.lock'))) return undefined;
  return false;
}

function shellChecks(source, file, firstLine, issues, fedora, toolVersions, toolChecksums, channels, root, sourceLines) {
  const parsed = commands(source, file, firstLine, sourceLines);
  const verified = new Set();
  const ambiguous = [];
  const cannotVerify = (tool, line) => {
    throw new InputError(problem(file, line, `cannot verify ${tool} invocation at ${file}:${line}; write it as a plain command`));
  };
  const unsupported = (token, detail) => {
    throw new InputError(problem(file, token?.line ?? firstLine, `unsupported construct: ${detail}`));
  };
  const literalPosition = token => {
    if (parsed.hasTools && /[$`]/.test(token?.value ?? '')) unsupported(token, 'dynamic command or tool action');
  };
  const verify = token => {
    if (token.value.includes('$') || token.value.includes('`')) cannotVerify(path.posix.basename(token.value), token.line);
    for (const occurrence of parsed.occurrences) {
      if (token.positions.includes(occurrence.offset)) verified.add(occurrence.offset);
    }
  };
  for (const raw of parsed) {
    let tokens = executable(withoutRedirections(raw, file));
    while (tokens.length) {
      literalPosition(tokens[0]);
      const command = path.posix.basename(tokens[0].value);
      if (command === 'rustup' && tokens[1]?.value === 'run') {
        const channel = tokens[2]?.value;
        if (!channel || channel.startsWith('-') || !tokens[3]) {
          throw new InputError(problem(file, tokens[0].line, 'use rustup run CHANNEL COMMAND without options'));
        }
        if (![...channels.values()].includes(channel)) issues.push(problem(file, tokens[0].line, `rustup run ${channel} must match a rust-toolchain.toml channel`));
        tokens = executable(tokens.slice(3));
        continue;
      }
      if (command === 'corepack') {
        literalPosition(tokens[1]);
        verify(tokens[0]);
        if (['pnpm', 'npm', 'npx', 'yarn'].includes(tokens[1]?.value)) { tokens = tokens.slice(1); continue; }
        if (!['enable', 'disable', 'pack', '--version', '-v', '--help', '-h'].includes(tokens[1]?.value)) {
          unsupported(tokens[1] ?? tokens[0], 'corepack action is not allowlisted');
        }
      }
      const manager = ['pnpm', 'npm', 'yarn'].includes(command);
      if (manager) literalPosition(tokens[1]);
      const action = manager ? packageAction(tokens) : undefined;
      if (manager && !action) cannotVerify(command, tokens[0].line);
      if (manager) {
        literalPosition(tokens[action.index]);
        if (action.action === undefined && command === 'yarn') action.action = 'install';
        const policy = packageActions[command];
        if (lockChangingActions.has(action.action)) {
          issues.push(problem(file, tokens[0].line, `${command} ${action.action} changes the lock; update dependencies outside CI`));
        } else if (![...policy.install, ...policy.allowed, ...policy.download].includes(action.action)) {
          unsupported(tokens[action.index] ?? tokens[0], `${command} action ${action.action ?? '(default)'} is not allowlisted`);
        }
      }
      if ((command === 'npx' && !['--version', '-v', '--help', '-h'].includes(tokens[1]?.value)) ||
          (manager && packageActions[command].download.includes(action.action))) {
        verify(tokens[0]);
        let index = command === 'npx' ? 1 : action.index + 1;
        if (['--', '--no-install'].includes(tokens[index]?.value)) index++;
        if (!tokens[index] || tokens[index].value.startsWith('-')) cannotVerify(command, tokens[0].line);
        literalPosition(tokens[index]);
        let spec = tokens[index].value;
        if (action?.action === 'create' || action?.action === 'init') {
          // create vite -> create-vite; @scope/name -> @scope/create-name.
          spec = spec.replace(/^(@[^/]+\/)?([^@/]+)(.*)$/, (_, scope = '', name, version) => `${scope}create-${name}${version}`);
        }
        const locked = downloadIsLocked(root, spec);
        if (locked !== false) ambiguous.push({ tool: command, line: tokens[0].line });
        else issues.push(problem(file, tokens[0].line, `${command} package ${spec} is not in a supported lockfile; use a locked local command`));
        tokens = executable(tokens.slice(index));
        continue;
      }
      if (['pnpm', 'yarn'].includes(command) && action?.action === 'exec') {
        verify(tokens[0]);
        let index = action.index + 1;
        if (tokens[index]?.value === '--') index++;
        if (!tokens[index] || tokens[index].value.startsWith('-')) cannotVerify(command, tokens[0].line);
        tokens = executable(tokens.slice(index));
        continue;
      }
      break;
    }
    const words = tokens.map(token => token.value);
    const command = path.posix.basename(words[0] ?? '');
    if (['cargo', 'pnpm', 'npm', 'npx', 'corepack', 'yarn'].includes(command)) verify(tokens[0]);
    if (command === 'cargo') {
      const channel = words[1]?.startsWith('+') ? words[1].slice(1) : undefined;
      if (channel !== undefined && ![...channels.values()].includes(channel)) {
        issues.push(problem(file, tokens[0].line, `cargo +${channel} must match a rust-toolchain.toml channel`));
      }
      const cargoWords = words.slice(0, words.includes('--') ? words.indexOf('--') : undefined);
      let actionIndex = channel === undefined ? 1 : 2;
      while (['--locked', '--frozen'].includes(cargoWords[actionIndex])) actionIndex++;
      const subcommand = cargoWords[actionIndex];
      literalPosition(tokens[1]);
      literalPosition(tokens[actionIndex]);
      // Do not guess whether an unknown global option consumes the action.
      if (subcommand?.startsWith('-') && !nonResolvingCargo.has(subcommand)) {
        unsupported(tokens[actionIndex], 'Cargo global options before the subcommand; place options after the action');
      }
      if (!nonResolvingCargo.has(subcommand) && !cargoWords.some(word => ['--locked', '--frozen'].includes(word))) {
        issues.push(problem(file, tokens[0].line, `cargo ${subcommand ?? '(default)'} requires --locked or --frozen before any -- separator`));
      }
    }
    if (command === 'pnpm') {
      const action = packageAction(tokens).action;
      const flags = words.slice(0, words.includes('--') ? words.indexOf('--') : undefined);
      const overridden = flags.some(word => /^--(?:no-frozen-lockfile|(?:config\.)?frozen-lockfile=false)$/.test(word));
      if (packageActions.pnpm.install.includes(action) && (!flags.includes('--frozen-lockfile') || overridden)) {
        issues.push(problem(file, tokens[0].line, `pnpm ${action} requires --frozen-lockfile without an override`));
      }
    }
    if (['pnpm', 'tauri'].includes(command) && words.includes('build') &&
        (command === 'tauri' || words.includes('tauri'))) {
      const separator = words.indexOf('--');
      if (separator < 0 || !words.slice(separator + 1).includes('--locked')) {
        issues.push(problem(file, tokens[0].line, 'Tauri build requires --locked forwarded to Cargo after --'));
      }
    }
    if (['npm', 'yarn'].includes(command) && packageActions[command].install.includes(packageAction(tokens).action ?? (command === 'yarn' ? 'install' : ''))) {
      const bootstrap = command === 'npm' && packageAction(tokens).action === 'install' &&
        words.some(word => ['-g', '--global'].includes(word)) &&
        words.filter(word => word.startsWith('corepack@')).length === 1 &&
        words.some(word => ['corepack@${COREPACK_VERSION}', 'corepack@$COREPACK_VERSION'].includes(word)) && toolVersions.has('COREPACK_VERSION') &&
        words.filter(word => !word.startsWith('-')).length === 3;
      if (!bootstrap) issues.push(problem(file, tokens[0].line, 'use pnpm install --frozen-lockfile (only the centrally pinned Corepack bootstrap is exempt)'));
    }
    if (['dnf', 'dnf5'].includes(command) && words.includes('install')) {
      for (const token of tokens) {
        const match = /^nodejs(?:(\d+)(?:-[A-Za-z0-9_-]+)?)?$/.exec(token.value);
        if (match) {
          if (!match[1]) issues.push(problem(file, token.line, 'Fedora Node package must name its major, for example nodejs24'));
          else fedora.push({ major: Number(match[1]), file, line: token.line });
        }
        if (token.value === 'npm') issues.push(problem(file, token.line, 'use the matching versioned Fedora Node npm package'));
      }
    }
    if (/--opt\s+context:/.test(words.join(' ')) || words.some(word => /^context:[^=]+=docker-image:\/\//.test(word))) {
      issues.push(problem(file, tokens[0]?.line ?? firstLine, 'image context overrides duplicate Dockerfile pins'));
    }
    for (const token of raw) {
      const assigned = /^([A-Za-z_][A-Za-z_0-9]*)=/.exec(token.value)?.[1];
      const value = assigned ? token.value.slice(assigned.length + 1) : undefined;
      const identity = assigned && !token.literalDollar && ['$' + assigned, '${' + assigned + '}'].includes(value);
      if (!identity && (toolVersions.has(assigned) || toolChecksums.has(assigned))) {
        issues.push(problem(file, token.line, `${assigned} is pinned again in a run script; use its env reference`));
      }
      if (!identity && /^(?:RUSTUP_TOOLCHAIN|RUST_NIGHTLY|NODE_IMAGE|RUST_IMAGE)=/.test(token.value)) {
        issues.push(problem(file, token.line, 'toolchain/image override duplicates its canonical file'));
      }
    }
  }
  for (const occurrence of parsed.occurrences) {
    if (!verified.has(occurrence.offset)) cannotVerify(occurrence.tool, occurrence.line);
  }
  if (ambiguous.length) cannotVerify(ambiguous[0].tool, ambiguous[0].line);
}

function pinRegistry({ issues, nodeMajors, rustImages }) {
  const pins = new Map();
  const register = (kind, identity, value, file, line, repeated = false) => {
    const key = `${kind}:${identity}`;
    if (pins.has(key) && (!repeated || pins.get(key) !== value)) issues.push(problem(file, line, `duplicate or conflicting ${kind} pin for ${identity}`));
    else pins.set(key, value);
  };
  const validateImage = (image, file, line) => {
    if (typeof image !== 'string' || !/^[^\s$]+@sha256:[a-f0-9]{64}$/.test(image)) {
      issues.push(problem(file, line, 'image must be literal and pinned by sha256 digest')); return;
    }
    const { name, tag } = imageParts(image);
    register('image', `${name}:${tag}`, image, file, line, true);
    if (/(?:^|\/)node$/.test(name)) {
      const match = /^(\d+)(?:[.-]|$)/.exec(tag);
      if (!match) issues.push(problem(file, line, 'Node image tag must name a numeric major'));
      else nodeMajors.push({ major: Number(match[1]), file, line });
    }
    if (/(?:^|\/)(?:rust|rustlang\/rust)$/.test(name)) rustImages.push({ tag, file, line });
  };
  return { register, validateImage };
}

function checkWorkflow(state, workflowFile) {
  const { root, issues, register, validateImage, fedora, channels } = state;
  const toolVersions = new Map(), toolChecksums = new Map();
  const workflow = yaml(path.join(root, workflowFile));
  const jobs = workflow.value?.jobs?.value;
  if (!jobs || Array.isArray(jobs) || typeof jobs !== 'object') throw new InputError(problem(workflowFile, 1, 'expected jobs mapping'));
  const required = path.posix.basename(workflowFile) === 'ci.yml';
  if (required && workflow.value.name?.value !== 'impluvium-ci') issues.push(problem(workflowFile, 1, 'workflow name must be impluvium-ci'));
  for (const name of required ? requiredJobs : []) {
    const job = jobs[name];
    if (!job) { issues.push(problem(workflowFile, 1, `required job ${name} is missing`)); continue; }
    if (typeof job.value !== 'object' || Array.isArray(job.value)) throw new InputError(problem(workflowFile, job.line, 'expected job mapping'));
    for (const key of ['if', 'name']) {
      if (job.value[key]) issues.push(problem(workflowFile, job.value[key].line, `${name}: required jobs cannot have job-level ${key}:`));
    }
    if (job.value.strategy?.value?.matrix) issues.push(problem(workflowFile, job.value.strategy.line, `${name}: required jobs cannot use a matrix`));
  }
  const centralEnv = workflow.value.env?.value ?? {};
  walkYaml(workflow, (node, keys) => {
    const last = keys.at(-1);
    if (keys.includes('strategy') && keys.includes('matrix') && typeof node.value === 'string' && hasToolWord(node.value)) {
      throw new InputError(problem(workflowFile, node.line, 'unsupported construct: strategy.matrix value mentions a tool word'));
    }
    if (last === 'uses') {
      if (typeof node.value !== 'string' || !/^[^\s@]+@[a-f0-9]{40}$/.test(node.value)) {
        if (typeof node.value === 'string' && node.value.startsWith('docker://')) validateImage(node.value.slice(9), workflowFile, node.line);
        else issues.push(problem(workflowFile, node.line, 'action must be pinned to a full 40-character commit SHA'));
      } else register('action', node.value.split('@')[0], node.value, workflowFile, node.line, true);
    }
    if ((last === 'container' && typeof node.value === 'string') || (last === 'image' && keys.includes('jobs'))) {
      validateImage(node.value, workflowFile, node.line);
    }
    if (keys.at(-2) === 'env') {
      if (/^(?:NODE_IMAGE|RUST_IMAGE|RUST_NIGHTLY|RUSTUP_TOOLCHAIN|NODE_VERSION|PNPM_VERSION|RUST_VERSION)$/.test(last)) issues.push(problem(workflowFile, node.line, `${last} duplicates a canonical image/version file`));
      const topLevel = keys.length === 2 && keys[0] === 'env';
      if (!topLevel && toolPin.test(last) && Object.hasOwn(centralEnv, last)) {
        issues.push(problem(workflowFile, node.line, `${last} must be defined once in workflow-level env`));
      }
      if (topLevel && toolPin.test(last)) {
        register('tool', last, node.value, workflowFile, node.line);
        if (last.endsWith('_VERSION')) {
          if (typeof node.value !== 'string' || !/^v?\d+\.\d+\.\d+(?:[-+][\w.-]+)?$/.test(node.value)) issues.push(problem(workflowFile, node.line, `${last} must be an exact version`));
          toolVersions.set(last, node.value);
        }
        if (last.endsWith('_SHA256')) {
          if (typeof node.value !== 'string' || !/^[a-f0-9]{64}$/.test(node.value)) issues.push(problem(workflowFile, node.line, `${last} must be a SHA256 checksum`));
          toolChecksums.set(last, node.value);
        }
      }
    }
  });
  for (const name of toolVersions.keys()) {
    if (name !== 'COREPACK_VERSION' && !toolChecksums.has(name.replace(/_VERSION$/, '_SHA256'))) issues.push(problem(workflowFile, 1, `${name} needs a matching SHA256 pin`));
  }
  for (const name of toolChecksums.keys()) {
    if (!toolVersions.has(name.replace(/_SHA256$/, '_VERSION'))) issues.push(problem(workflowFile, 1, `${name} needs a matching version pin`));
  }
  walkYaml(workflow, (node, keys) => {
    if (keys.at(-1) === 'run') {
      if (typeof node.value !== 'string') throw new InputError(problem(workflowFile, node.line, 'run must contain a shell script'));
      const expression = /\$\{\{\s*(?:matrix|inputs)\b/.exec(node.value);
      if (expression) throw new InputError(problem(workflowFile, node.line + lineAt(node.value, expression.index) - 1,
        'unsupported construct: matrix/inputs expressions in run scripts'));
      shellChecks(node.value, workflowFile, node.line, issues, fedora, toolVersions, toolChecksums, channels, root);
    }
  });

  // Prevent additional version/hash literals from becoming a second tool pin.
  walkYaml(workflow, (node, keys) => {
    if (keys.at(-1) !== 'run' || typeof node.value !== 'string') return;
    for (const [name, value] of [...toolVersions, ...toolChecksums]) {
      if (typeof value !== 'string') continue;
      const escaped = value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const match = new RegExp(`(^|[^\\w.])${escaped}(?![\\w.])`).exec(node.value);
      if (match) issues.push(problem(workflowFile, node.line + lineAt(node.value, match.index) - 1, `${name} is pinned again in a run script; use its env reference`));
    }
  });
  for (const [name, value] of toolVersions) state.toolVersions.set(name, value);
  for (const [name, value] of toolChecksums) state.toolChecksums.set(name, value);
}

// Docker option values may be quoted; retain the remaining shell/JSON body.
function dockerOptions(body, file, line) {
  const options = [];
  let rest = body.trimStart();
  while (rest.startsWith('--')) {
    const match = /^--([a-z-]+)(?:=(?:"([^"]*)"|'([^']*)'|([^\s]+)))?(?=\s|$)\s*/.exec(rest);
    if (!match) throw new InputError(problem(file, line, 'unsupported Docker instruction option syntax'));
    options.push({ name: match[1], value: match[2] ?? match[3] ?? match[4], offset: body.length - rest.length });
    rest = rest.slice(match[0].length);
  }
  return { options, rest, restOffset: body.length - rest.length };
}

function checkDockerfile(state) {
  const { root, dockerFile, issues, validateImage, fedora, toolVersions, toolChecksums, channels } = state;
  const docker = read(path.join(root, dockerFile));
  const syntax = /^\s*#\s*syntax\s*=\s*(\S+)/gim;
  for (const match of docker.matchAll(syntax)) validateImage(match[1], dockerFile, lineAt(docker, match.index));
  const stages = new Set();
  const args = new Map();
  const argValues = new Map();
  const usedArgs = new Set();
  let beforeFrom = true;
  let stageCount = 0;
  const imageSource = (source, line) => {
    const variable = /^\$(?:\{([A-Za-z_]\w*)\}|([A-Za-z_]\w*))$/.exec(source);
    if (variable) {
      const name = variable[1] ?? variable[2];
      if (!args.has(name)) { issues.push(problem(dockerFile, line, `${name} needs a digest-pinned ARG default`)); return; }
      usedArgs.add(name);
      validateImage(args.get(name).value, dockerFile, args.get(name).line);
    } else if (!stages.has(source.toLowerCase())) validateImage(source, dockerFile, line);
  };
  // Docker removes backslash-newline before interpreting instruction options.
  // Map every retained character back to its physical line for diagnostics.
  let instruction = '', instructionLine = 1, instructionLines = [];
  const instructions = [];
  for (const [index, line] of docker.split('\n').entries()) {
    if (!line.trim() || /^\s*#/.test(line)) continue;
    if (!instruction) instructionLine = index + 1;
    const continued = /\\\s*$/.test(line);
    const text = continued ? line.replace(/\\\s*$/, '') : line;
    instruction += text;
    instructionLines.push(...Array(text.length).fill(index + 1));
    if (!continued) {
      instructions.push({ text: instruction, line: instructionLine, lines: instructionLines });
      instruction = ''; instructionLines = [];
    }
  }
  if (instruction) throw new InputError(problem(dockerFile, instructionLine, 'unfinished Dockerfile continuation'));
  for (const item of instructions) {
    const match = /^\s*(ARG|FROM|RUN|ENV|COPY|ADD|CMD|ENTRYPOINT|EXPOSE|HEALTHCHECK|LABEL|MAINTAINER|ONBUILD|SHELL|STOPSIGNAL|USER|VOLUME|WORKDIR)\s+([\s\S]*)$/i.exec(item.text);
    if (!match) throw new InputError(problem(dockerFile, item.line, 'expected a known Dockerfile instruction'));
    const op = match[1].toUpperCase(), body = match[2];
    const bodyOffset = item.text.length - body.length;
    const location = offset => item.lines[bodyOffset + offset] ?? item.line;
    if (['ONBUILD', 'SHELL'].includes(op)) throw new InputError(problem(dockerFile, item.line, `${op} instructions are not supported; use ordinary shell-form RUN`));
    if (op === 'ARG') {
      const arg = /^([A-Za-z_][A-Za-z_0-9]*)(?:=(.*))?$/.exec(body);
      if (!arg) throw new InputError(problem(dockerFile, item.line, 'malformed ARG'));
      if (arg[2] !== undefined) {
        if (toolVersions.has(arg[1]) || toolChecksums.has(arg[1])) issues.push(problem(dockerFile, item.line, `${arg[1]} must be pinned only in workflow-level env`));
        if (args.has(arg[1])) issues.push(problem(dockerFile, item.line, `duplicate image ARG ${arg[1]}`));
        argValues.set(arg[1], arg[2].replace(/^(['"])(.*)\1$/, '$2'));
      }
      if (arg[2]?.includes('@sha256:')) {
        if (!beforeFrom) issues.push(problem(dockerFile, item.line, 'image ARG defaults must precede the first FROM'));
        args.set(arg[1], { value: arg[2].replace(/^(['"])(.*)\1$/, '$2'), line: item.line });
      }
    }
    if (op === 'FROM') {
      beforeFrom = false;
      const from = /^(?:--platform=\S+\s+)?(\S+)(?:\s+AS\s+(\S+))?\s*$/i.exec(body);
      if (!from) throw new InputError(problem(dockerFile, item.line, 'unsupported FROM syntax'));
      let image = from[1];
      const variable = /^\$(?:\{([A-Za-z_]\w*)\}|([A-Za-z_]\w*))$/.exec(image);
      if (variable) {
        const name = variable[1] ?? variable[2];
        if (!args.has(name)) issues.push(problem(dockerFile, item.line, `${name} needs a digest-pinned ARG default`));
        else {
          image = args.get(name).value;
          if (!usedArgs.has(name)) validateImage(image, dockerFile, args.get(name).line);
          usedArgs.add(name);
        }
      } else if (!stages.has(image.toLowerCase()) && image !== 'scratch') validateImage(image, dockerFile, item.line);
      stages.add(String(stageCount++));
      if (from[2]) stages.add(from[2].toLowerCase());
    }
    if (op === 'COPY') {
      for (const option of dockerOptions(body, dockerFile, item.line).options) {
        if (option.name !== 'from') continue;
        if (!option.value) throw new InputError(problem(dockerFile, item.line, 'use COPY --from=SOURCE'));
        imageSource(option.value, location(option.offset));
      }
    }
    if (op === 'RUN') {
      const parsed = dockerOptions(body, dockerFile, item.line);
      for (const option of parsed.options) {
        if (!['mount', 'network', 'security'].includes(option.name) || !option.value) {
          throw new InputError(problem(dockerFile, item.line, 'unsupported RUN option; use --mount=, --network= or --security='));
        }
        if (option.name === 'mount') {
          for (const field of option.value.split(',')) {
            if (field.startsWith('from=')) imageSource(field.slice(5).replace(/^(['"])(.*)\1$/, '$2'), location(option.offset));
          }
        }
      }
      const script = parsed.rest;
      if (script.startsWith('[') || /<<[-~]?\s*['"]?\w/.test(script)) throw new InputError(problem(dockerFile, item.line, 'use ordinary shell-form RUN without a Dockerfile here-document'));
      shellChecks(script, dockerFile, location(parsed.restOffset), issues, fedora, toolVersions, toolChecksums, channels, root,
        item.lines.slice(bodyOffset + parsed.restOffset));
    }
    if (op === 'ENV') {
      for (const name of [...toolVersions.keys(), ...toolChecksums.keys()]) {
        if (new RegExp(`(?:^|\\s)${name}(?:=|\\s)`).test(body)) issues.push(problem(dockerFile, item.line, `${name} must be pinned only in workflow-level env`));
      }
    }
    if (op === 'ENV' && /\bRUSTUP_TOOLCHAIN(?:=|\s)/.test(body)) {
      let value = /\bRUSTUP_TOOLCHAIN(?:=|\s+)(["']?)([^\s"']+)\1/.exec(body)?.[2];
      const variable = /^\$(?:\{(\w+)\}|(\w+))$/.exec(value ?? '');
      if (variable) value = argValues.get(variable[1] ?? variable[2]);
      const rootChannel = toml(path.join(root, 'rust-toolchain.toml')).data.toolchain?.channel;
      if (!value || value !== rootChannel) issues.push(problem(dockerFile, item.line, 'RUSTUP_TOOLCHAIN must match rust-toolchain.toml'));
    }
  }
  for (const [name, arg] of args) if (!usedArgs.has(name)) issues.push(problem(dockerFile, arg.line, `unused image pin ${name}`));

}

function checkPackages(state) {
  const { root, allFiles, issues, fedora, nodeMajors, toolVersions, toolChecksums, channels, dockerFile, workflowFile } = state;
  const nodeFile = '.node-version';
  const nodeVersion = read(path.join(root, nodeFile)).trim();
  if (!/^\d+$/.test(nodeVersion)) throw new InputError(problem(nodeFile, 1, 'expected a single Node major'));
  const major = Number(nodeVersion);
  if (major !== 24) issues.push(problem(nodeFile, 1, 'CI tools require Node major 24'));
  for (const file of allFiles) {
    if (path.basename(file) === '.nvmrc' || (path.basename(file) === '.node-version' && file !== nodeFile)) {
      issues.push(problem(file, 1, 'Node version belongs only in root .node-version'));
    }
  }
  for (const file of allFiles.filter(file => path.basename(file) === 'package.json')) {
    const absolute = path.join(root, file), source = read(absolute), data = json(absolute);
    if (file === 'package.json' && (typeof data.packageManager !== 'string' || !/^pnpm@\d+\.\d+\.\d+\+sha(?:224|256|384|512)\.[a-f0-9]+$/.test(data.packageManager))) {
      issues.push(problem(file, keyLine(source, 'packageManager'), 'root packageManager must pin pnpm exactly with an integrity hash'));
    }
    if (file !== 'package.json' && data.packageManager) issues.push(problem(file, keyLine(source, 'packageManager'), 'packageManager must be defined only in root package.json'));
    const engine = data.engines?.node;
    if (engine !== undefined) {
      // A major, exact version, caret/tilde in that major, or an explicitly
      // bounded same-major range. Open-ended >= ranges also allow future majors.
      const text = String(engine).trim();
      const exact = /^(?:[~^]?)(\d+)(?:\.(?:\d+|x|\*)){0,2}$/.exec(text);
      const bounded = /^>=\s*(\d+)(?:\.\d+){0,2}\s+<\s*(\d+)(?:\.0){0,2}$/.exec(text);
      const sameMajor = exact && Number(exact[1]) === major;
      const boundedMajor = bounded && Number(bounded[1]) === major && Number(bounded[2]) === major + 1;
      if (!sameMajor && !boundedMajor) {
        issues.push(problem(file, keyLine(source, 'node'), `engines.node must constrain Node to major ${major}`));
      }
    }
    for (const section of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies']) {
      for (const [name, version] of Object.entries(data[section] ?? {})) {
        if (typeof version !== 'string' || /^(?:latest|\*|>=)/.test(version)) issues.push(problem(file, keyLine(source, name), `${name}: dependency must have a bounded version`));
      }
    }
    for (const [name, script] of Object.entries(data.scripts ?? {})) {
      if (typeof script !== 'string') throw new InputError(problem(file, keyLine(source, name), 'package script must be a string'));
      shellChecks(script, file, keyLine(source, name), issues, fedora, toolVersions, toolChecksums, channels, root);
    }
  }
  if (json(path.join(root, 'package.json')).engines?.node === undefined) issues.push(problem('package.json', 1, 'root engines.node must specify the Node major'));
  const cargo = manifests(root);
  for (const manifest of cargo.filter(item => !item.relative.startsWith('server/deploy-template/'))) {
    const workspaceDependencies = Object.entries(manifest.data.workspace?.dependencies ?? {}).map(([alias, value]) => ({
      alias, value, line: manifest.entries.find(item => item.keys[0] === 'workspace' && item.keys[2] === alias)?.line ?? 1,
    }));
    for (const dependency of [...dependencies(manifest), ...workspaceDependencies]) {
      const version = typeof dependency.value === 'string' ? dependency.value : dependency.value?.version;
      if (typeof version === 'string' && /\*|>=/.test(version)) issues.push(problem(manifest.relative, dependency.line, `${dependency.alias}: Cargo dependency must have a bounded version`));
    }
  }
  if (!nodeMajors.length) issues.push(problem(dockerFile, 1, 'no tagged Node image found'));
  if (!fedora.length) issues.push(problem(workflowFile, 1, 'no versioned Fedora Node package found'));
  for (const pin of [...nodeMajors, ...fedora]) if (pin.major !== major) issues.push(problem(pin.file, pin.line, `Node major ${pin.major} differs from ${nodeFile} (${major})`));

}

function checkToolchains({ root, allFiles, issues, channels }) {
  for (const file of allFiles.filter(file => path.basename(file) === 'rust-toolchain.toml')) {
    const parsed = toml(path.join(root, file));
    const channel = parsed.data.toolchain?.channel;
    const line = parsed.entries.find(item => item.keys.join('.') === 'toolchain.channel')?.line ?? 1;
    if (typeof channel !== 'string' || !/^(?:\d+\.\d+\.\d+|nightly-\d{4}-\d{2}-\d{2})$/.test(channel)) issues.push(problem(file, line, 'toolchain must be an exact stable release or dated nightly'));
    channels.set(file, channel);
  }
  if (!channels.has('rust-toolchain.toml')) throw new InputError(problem('rust-toolchain.toml', 1, 'root toolchain file is required'));
}

export function check(options) {
  const root = path.resolve(options.root ?? '.');
  const state = {
    root, allFiles: files(root).filter(file => !file.startsWith('server/deploy-template/')),
    workflowFile: '.gitea/workflows/ci.yml', dockerFile: 'Dockerfile', issues: [],
    nodeMajors: [], rustImages: [], fedora: [], toolVersions: new Map(), toolChecksums: new Map(), channels: new Map(),
  };
  Object.assign(state, pinRegistry(state));
  checkToolchains(state);
  const workflows = state.allFiles.filter(file => /^(?:\.gitea|\.github)\/workflows\//.test(file));
  if (!workflows.includes(state.workflowFile)) throw new InputError(problem(state.workflowFile, 1, 'required workflow file is missing'));
  for (const file of workflows) checkWorkflow(state, file);
  checkDockerfile(state);
  checkPackages(state);
  const { issues, rustImages, channels, dockerFile } = state;
  if (!rustImages.length) issues.push(problem(dockerFile, 1, 'no Rust image found'));
  for (const image of rustImages) {
    const channel = channels.get('rust-toolchain.toml');
    if (typeof channel !== 'string' || !(image.tag === channel || image.tag.startsWith(channel + '-'))) issues.push(problem(image.file, image.line, `Rust image tag must match root toolchain ${channel}`));
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-pins.mjs [--root DIR]
Check every file under .gitea/workflows/ and .github/workflows/, Dockerfile, toolchains and package/Cargo manifests.
Require digest images, commit actions, centralized tool pins, locked Cargo commands, frozen pnpm installs,
stable required job IDs, matching Rust image/toolchain and Node 24 across image/package/engines inputs.
Deployment templates and generated trees are excluded. See tools/ci/README.md for supported input syntax.
Exit: 0 passed, 1 policy violation, 2 usage or input error.`, { root: stringOption }, check);
