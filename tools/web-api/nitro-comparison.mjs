// These types are inspected by the compiler; no server module is imported at runtime.
export const comparisonTypes = `
type WireJsonValue = string | number | boolean | null | WireJsonValue[] | WireJsonObject;
type WireJsonObject = { [key: string]: WireJsonValue };
`;

export function comparisonResponse(route, index) {
  const module = `./${route.file.slice('server/'.length).replace(/\.ts$/, '')}`;
  return { declarations: [`type Raw${index} = Awaited<ReturnType<typeof import(${JSON.stringify(module)}).default>>;`] };
}

export function responseMismatch(route, problems) {
  return `HTTP response contract: ${route}\nResponse mismatch:\n${problems.map((problem) => `  ${problem}`).join('\n')}`;
}

/** Compare wire shapes independently of the DTO materializer and route names. */
export function responseComparator(ts, checker, aliases) {
  const jsonObject = aliases.get('WireJsonObject');
  const parts = (type) => type.isUnion() ? type.types : [type];
  const absent = (type) => !!(type.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Void | ts.TypeFlags.Never));
  const equivalent = (left, right) => checker.isTypeAssignableTo(left, right) && checker.isTypeAssignableTo(right, left);
  const propertyType = (property) => checker.getTypeOfSymbol(property);
  const rawParts = (hints) => hints.flatMap(parts).filter((type) => !absent(type));
  const rawField = (hints, key) => rawParts(hints).flatMap((type) => {
    const property = type.getProperty(key);
    return property ? [propertyType(property)] : [];
  });
  const rawElements = (hints) => rawParts(hints).flatMap((type) =>
    checker.isArrayType(type) || checker.isTupleType(type) ? checker.getTypeArguments(type) : []);
  const rawTupleElement = (hints, index) => rawParts(hints).flatMap((type) => {
    if (checker.isTupleType(type)) return checker.getTypeArguments(type).slice(index, index + 1);
    return checker.isArrayType(type) ? checker.getTypeArguments(type) : [];
  });

  // Assignability alone accepts any inside arrays, dictionaries and unions.
  // Inspect every leaf, using a visited set for recursive JSON and schema types.
  function requireTyped(type, path, seen = new Set()) {
    if (type.flags & (ts.TypeFlags.Any | ts.TypeFlags.Unknown)) {
      throw new Error(`${path}: unresolved ${checker.typeToString(type)}`);
    }
    if (seen.has(type)) return;
    seen.add(type);
    if (type.isUnion()) {
      for (const member of type.types) requireTyped(member, path, seen);
    } else if (checker.isArrayType(type) || checker.isTupleType(type)) {
      for (const element of checker.getTypeArguments(type)) requireTyped(element, `${path}[]`, seen);
    } else if (type.flags & (ts.TypeFlags.Object | ts.TypeFlags.Intersection)) {
      for (const property of checker.getPropertiesOfType(type)) {
        if (property.name.startsWith('__@')) continue;
        const value = propertyType(property);
        if (!value.getCallSignatures().length) requireTyped(value, `${path}.${property.name}`, seen);
      }
      for (const index of checker.getIndexInfosOfType(type)) requireTyped(index.type, `${path}[key]`, seen);
    }
  }

  function shapes() {
    const cache = new Map();
    const ids = new Map();
    const id = (type) => {
      if (!ids.has(type)) ids.set(type, ids.size);
      return ids.get(type);
    };
    function shape(type, hints, path, position = 'value') {
      if (type.flags & (ts.TypeFlags.Any | ts.TypeFlags.Unknown)) {
        throw new Error(`${path}: unresolved ${checker.typeToString(type)}`);
      }
      const key = `${id(type)}:${[...new Set(hints.map(id))].sort((a, b) => a - b).join(',')}:${position}`;
      if (cache.has(key)) return cache.get(key);
      const node = {};
      cache.set(key, node);
      const finish = (value) => Object.assign(node, value);

      const indices = checker.getIndexInfosOfType(type);
      if (type.isUnion()) {
        return finish({ kind: 'union', members: type.types.map((member) => shape(member, hints, path, position)) });
      }
      const flags = type.flags;
      if (flags & ts.TypeFlags.Never) return finish({ kind: 'never' });
      if (flags & (ts.TypeFlags.Null | ts.TypeFlags.Undefined | ts.TypeFlags.Void)) {
        if (position === 'root') return finish({ kind: 'empty' });
        return finish({ kind: (flags & ts.TypeFlags.Null) || position === 'array' ? 'null' : 'undefined' });
      }
      // String enums carry nominal TS identities, which JSON does not retain.
      // Their literal values remain exact, including inside arrays and objects.
      if (flags & ts.TypeFlags.StringLiteral) return finish({ kind: 'literal', value: type.value });
      if (flags & ts.TypeFlags.NumberLiteral) return finish({ kind: 'literal', value: type.value });
      if (flags & ts.TypeFlags.BooleanLiteral) return finish({ kind: 'literal', value: type.intrinsicName === 'true' });
      if (flags & ts.TypeFlags.String) return finish({ kind: 'string' });
      if (flags & ts.TypeFlags.Number) return finish({ kind: 'number' });
      if (flags & ts.TypeFlags.Boolean) return finish({ kind: 'union', members: [{ kind: 'literal', value: false }, { kind: 'literal', value: true }] });
      if (checker.isTupleType(type)) {
        const elements = checker.getTypeArguments(type);
        return finish({ kind: 'tuple', elements: elements.map((element, index) => ({
          flags: type.target.elementFlags[index],
          value: shape(element, rawTupleElement(hints, index), `${path}[${index}]`, 'array'),
        })) });
      }
      if (checker.isArrayType(type)) {
        return finish({ kind: 'array', element: shape(checker.getTypeArguments(type)[0], rawElements(hints), `${path}[]`, 'array') });
      }
      if (!(flags & (ts.TypeFlags.Object | ts.TypeFlags.Intersection)) || type.getCallSignatures().length) {
        throw new Error(`${path}: unsupported ${checker.typeToString(type)}`);
      }
      const properties = checker.getPropertiesOfType(type).filter((property) => !property.name.startsWith('__@'));
      // Serialize<object> becomes {}. Recover its open JSON-object domain only
      // with evidence of the exact non-primitive object keyword in the original
      // handler type. Neither an empty schema nor any provides that evidence.
      const objectHints = rawParts(hints).filter((hint) =>
        hint.flags & (ts.TypeFlags.Object | ts.TypeFlags.Intersection | ts.TypeFlags.NonPrimitive) &&
        !checker.isArrayType(hint) && !checker.isTupleType(hint) && !hint.getCallSignatures().length);
      if (!properties.length && !indices.length && objectHints.length && objectHints.every((hint) => hint.flags & ts.TypeFlags.NonPrimitive)) {
        return finish(shape(jsonObject, [], path));
      }
      const fields = new Map();
      finish({ kind: 'object', fields, indices: new Map() });
      for (const property of properties.sort((a, b) => a.name.localeCompare(b.name, 'en'))) {
        const value = propertyType(property);
        if (value.getCallSignatures().length) continue;
        const members = parts(value).filter((member) => !absent(member));
        // JSON omits undefined-only fields, including optional never fields.
        // Other undefined unions become optional; their remaining values and
        // every field name are still compared exactly.
        if (!members.length) continue;
        fields.set(property.name, {
          optional: !!(property.flags & ts.SymbolFlags.Optional) || parts(value).some(absent),
          value: { kind: 'union', members: members.map((member) => shape(member, rawField(hints, property.name), `${path}.${property.name}`)) },
        });
      }
      for (const index of indices) {
        // This also handles Prisma's optional JSON index values at every
        // nesting depth. Omit undefined entries, then compare the complete
        // remaining value domain; never replace a schema with arbitrary JSON.
        const members = parts(index.type).filter((member) => !absent(member));
        if (!members.length) continue;
        if (!(index.keyType.flags & (ts.TypeFlags.Number | ts.TypeFlags.String))) {
          throw new Error(`${path}: unsupported index key ${checker.typeToString(index.keyType)}`);
        }
        const keyKind = index.keyType.flags & ts.TypeFlags.Number ? 'number' : 'string';
        const rawIndices = rawParts(hints).flatMap((hint) => checker.getIndexInfosOfType(hint)
          .filter((rawIndex) => equivalent(rawIndex.keyType, index.keyType)).map((rawIndex) => rawIndex.type));
        node.indices.set(keyKind, { kind: 'union', members: members.map((member) => shape(member, rawIndices, `${path}[${keyKind}]`)) });
      }
      return node;
    }
    return shape;
  }

  function differences(left, right, path = '$', seen = new Map()) {
    if (left === right || seen.get(left)?.has(right)) return [];
    const next = new Map(seen);
    next.set(left, new Set([...(seen.get(left) ?? []), right]));
    if (left.kind === 'union' && left.members.length === 1) return differences(left.members[0], right, path, next);
    if (right.kind === 'union' && right.members.length === 1) return differences(left, right.members[0], path, next);
    if (left.kind === 'union' || right.kind === 'union') {
      const a = left.kind === 'union' ? left.members : [left];
      const b = right.kind === 'union' ? right.members : [right];
      if (a.every((member) => b.some((other) => !differences(member, other, path, next).length)) &&
          b.every((member) => a.some((other) => !differences(other, member, path, next).length))) return [];
      return [`${path}: different value domains`];
    }
    if (left.kind !== right.kind) return [`${path}: contract ${left.kind}, Nitro ${right.kind}`];
    if (left.kind === 'literal') return left.value === right.value ? [] : [`${path}: different literal values`];
    if (left.kind === 'array') return differences(left.element, right.element, `${path}[]`, next);
    if (left.kind === 'tuple') {
      if (left.elements.length !== right.elements.length) return [`${path}: different tuple lengths`];
      return left.elements.flatMap((element, index) => element.flags !== right.elements[index].flags
        ? [`${path}[${index}]: different tuple presence`]
        : differences(element.value, right.elements[index].value, `${path}[${index}]`, next));
    }
    if (left.kind !== 'object') return [];
    const errors = [];
    for (const key of new Set([...left.fields.keys(), ...right.fields.keys()])) {
      const a = left.fields.get(key);
      const b = right.fields.get(key);
      if (!a || !b) errors.push(`${path}.${key}: field missing from ${a ? 'Nitro' : 'contract'}`);
      else {
        if (a.optional !== b.optional) errors.push(`${path}.${key}: different field presence`);
        errors.push(...differences(a.value, b.value, `${path}.${key}`, next));
      }
    }
    // Nitro's mapped SerializeObject may add a numeric index to a string
    // dictionary: JavaScript coerces numeric object keys to strings. It is
    // redundant only when both indices have exactly the same wire domain.
    // A narrower numeric index remains visible and must still match.
    const indices = (node) => {
      const result = new Map(node.indices);
      if (result.has('string') && result.has('number') &&
          !differences(result.get('string'), result.get('number'), `${path}[number]`, next).length) {
        result.delete('number');
      }
      return result;
    };
    const leftIndices = indices(left);
    const rightIndices = indices(right);
    for (const key of new Set([...leftIndices.keys(), ...rightIndices.keys()])) {
      const a = leftIndices.get(key);
      const b = rightIndices.get(key);
      if (!a || !b) errors.push(`${path}[${key}]: different index signatures`);
      else errors.push(...differences(a, b, `${path}[${key}]`, next));
    }
    return errors;
  }

  return (contract, observed, { raw, kind = 'json' } = {}) => {
    try {
      requireTyped(contract, '$');
      requireTyped(observed, '$');
      // Nitro 2 serializes void to never. Only a route explicitly classified
      // as empty whose original handler is provably void/undefined may bridge
      // that result. Throwing, untyped and JSON handlers remain mismatches.
      const voidBody = raw && parts(raw).every((member) => member.flags & (ts.TypeFlags.Void | ts.TypeFlags.Undefined));
      const shape = shapes();
      const actual = kind === 'empty' && (observed.flags & ts.TypeFlags.Never) && voidBody
        ? { kind: 'empty' } : shape(observed, raw ? [raw] : [], '$', 'root');
      return differences(shape(contract, [], '$', 'root'), actual);
    } catch (error) {
      return [error.message];
    }
  };
}
