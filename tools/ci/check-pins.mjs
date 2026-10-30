import path from 'node:path';
import { cli, files, InputError, json, keyLine, lineAt, problem, read, stringOption } from './common.mjs';
import { dependencies, manifests } from './cargo.mjs';
import { commands, executable } from './shell.mjs';
import { toml } from './toml.mjs';
import { walkYaml, yaml } from './yaml.mjs';

const requiredJobs = ['checks', 'rust', 'server', 'desktop-ui', 'server-image', 'client-linux'];
const resolving = new Set(['build', 'check', 'test', 'clippy', 'run', 'metadata', 'tree', 'install', 'deny',
  'bench', 'doc', 'rustc', 'rustdoc', 'fix', 'fetch', 'generate-lockfile', 'update', 'package', 'publish']);
const toolPin = /^(?:[A-Z][A-Z_0-9]*_(?:VERSION|SHA256)|RUST_NIGHTLY|RUSTUP_TOOLCHAIN)$/;

function imageParts(image) {
  const withoutDigest = image.split('@')[0];
  const slash = withoutDigest.lastIndexOf('/');
  const colon = withoutDigest.lastIndexOf(':');
  return { name: colon > slash ? withoutDigest.slice(0, colon) : withoutDigest,
    tag: colon > slash ? withoutDigest.slice(colon + 1) : '' };
}

function shellChecks(source, file, firstLine, issues, fedora, toolVersions, toolChecksums, channels) {
  for (const raw of commands(source, file, firstLine)) {
    let tokens = executable(raw);
    while (path.posix.basename(tokens[0]?.value ?? '') === 'rustup' && tokens[1]?.value === 'run') {
      const channel = tokens[2]?.value;
      if (!channel || channel.startsWith('-') || !tokens[3]) {
        throw new InputError(problem(file, tokens[0].line, 'use rustup run CHANNEL COMMAND without options'));
      }
      if (![...channels.values()].includes(channel)) issues.push(problem(file, tokens[0].line, `rustup run ${channel} must match a rust-toolchain.toml channel`));
      tokens = executable(tokens.slice(3));
    }
    if (['time', 'timeout', 'nice', 'xargs'].includes(path.posix.basename(tokens[0]?.value ?? ''))) {
      throw new InputError(problem(file, tokens[0].line, `unsupported shell wrapper ${tokens[0].value}; use an explicit command`));
    }
    const words = tokens.map(token => token.value);
    const command = path.posix.basename(words[0] ?? '');
    if (command === 'cargo') {
      const channel = words[1]?.startsWith('+') ? words[1].slice(1) : undefined;
      if (channel !== undefined && ![...channels.values()].includes(channel)) {
        issues.push(problem(file, tokens[0].line, `cargo +${channel} must match a rust-toolchain.toml channel`));
      }
      const cargoWords = words.slice(0, words.includes('--') ? words.indexOf('--') : undefined);
      const subcommand = cargoWords.find(word => resolving.has(word));
      if (subcommand && !cargoWords.includes('--locked')) issues.push(problem(file, tokens[0].line, `cargo ${subcommand} requires --locked before any -- separator`));
    }
    if (command === 'pnpm') {
      const action = words.find(word => ['install', 'i', 'add', 'update', 'up'].includes(word));
      const flags = words.slice(0, words.includes('--') ? words.indexOf('--') : undefined);
      const overridden = flags.some(word => /^--(?:no-frozen-lockfile|(?:config\.)?frozen-lockfile=false)$/.test(word));
      if (action && (!flags.includes('--frozen-lockfile') || overridden)) {
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
    if (['npm', 'yarn'].includes(command) && words.some(word => ['install', 'i', 'ci', 'add'].includes(word))) {
      const bootstrap = command === 'npm' && words.includes('install') &&
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
      if (toolVersions.has(assigned) || toolChecksums.has(assigned)) {
        issues.push(problem(file, token.line, `${assigned} is pinned again in a run script; use its env reference`));
      }
      if (/^(?:RUSTUP_TOOLCHAIN|RUST_NIGHTLY|NODE_IMAGE|RUST_IMAGE)=/.test(token.value)) {
        issues.push(problem(file, token.line, 'toolchain/image override duplicates its canonical file'));
      }
    }
  }
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
    register('image', tag ? `${name}:${tag}` : image, image, file, line, true);
    if (/(?:^|\/)node$/.test(name)) {
      const match = /^(\d+)(?:[.-]|$)/.exec(tag);
      if (!match) issues.push(problem(file, line, 'Node image tag must name a numeric major'));
      else nodeMajors.push({ major: Number(match[1]), file, line });
    }
    if (/(?:^|\/)(?:rust|rustlang\/rust)$/.test(name)) rustImages.push({ tag, file, line });
  };
  return { register, validateImage };
}

function checkWorkflow(state) {
  const { root, workflowFile, issues, register, validateImage, fedora, toolVersions, toolChecksums, channels } = state;
  const workflow = yaml(path.join(root, workflowFile));
  const jobs = workflow.value?.jobs?.value;
  if (!jobs || Array.isArray(jobs) || typeof jobs !== 'object') throw new InputError(problem(workflowFile, 1, 'expected jobs mapping'));
  if (workflow.value.name?.value !== 'impluvium-ci') issues.push(problem(workflowFile, 1, 'workflow name must be impluvium-ci'));
  for (const name of requiredJobs) {
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
      shellChecks(node.value, workflowFile, node.line, issues, fedora, toolVersions, toolChecksums, channels);
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
}

// Docker option values may be quoted; retain the remaining shell/JSON body.
function dockerOptions(body, file, line) {
  const options = [];
  let rest = body.trimStart();
  while (rest.startsWith('--')) {
    const match = /^--([a-z-]+)(?:=(?:"([^"]*)"|'([^']*)'|([^\s]+)))?(?=\s|$)\s*/.exec(rest);
    if (!match) throw new InputError(problem(file, line, 'unsupported Docker instruction option syntax'));
    options.push({ name: match[1], value: match[2] ?? match[3] ?? match[4] });
    rest = rest.slice(match[0].length);
  }
  return { options, rest };
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
  // Keep a separate physical-line counter when joining continued instructions.
  let instruction = '', instructionLine = 1;
  const instructions = [];
  for (const [index, line] of docker.split('\n').entries()) {
    if (!instruction) instructionLine = index + 1;
    if (/\\\s*$/.test(line)) instruction += line.replace(/\\\s*$/, ' ');
    else { instructions.push({ text: instruction + line, line: instructionLine }); instruction = ''; }
  }
  if (instruction) throw new InputError(problem(dockerFile, instructionLine, 'unfinished Dockerfile continuation'));
  for (const item of instructions) {
    const match = /^\s*(ARG|FROM|RUN|ENV|COPY)\s+([\s\S]*)$/i.exec(item.text);
    if (!match) continue;
    const op = match[1].toUpperCase(), body = match[2];
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
        imageSource(option.value, item.line);
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
            if (field.startsWith('from=')) imageSource(field.slice(5).replace(/^(['"])(.*)\1$/, '$2'), item.line);
          }
        }
      }
      const script = parsed.rest;
      if (script.startsWith('[') || /<<[-~]?\s*['"]?\w/.test(script)) throw new InputError(problem(dockerFile, item.line, 'use ordinary shell-form RUN without a Dockerfile here-document'));
      shellChecks(script, dockerFile, item.line, issues, fedora, toolVersions, toolChecksums, channels);
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
      shellChecks(script, file, keyLine(source, name), issues, fedora, toolVersions, toolChecksums, channels);
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
  checkWorkflow(state);
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
Check .gitea/workflows/ci.yml, Dockerfile, toolchains and package/Cargo manifests.
Require digest images, commit actions, centralized tool pins, locked Cargo commands, frozen pnpm installs,
stable required job IDs, matching Rust image/toolchain and Node 24 across image/package/engines inputs.
Deployment templates and generated trees are excluded. See tools/ci/README.md for supported input syntax.
Exit: 0 passed, 1 policy violation, 2 usage or input error.`, { root: stringOption }, check);
