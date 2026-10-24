import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { buildSchema, Kind, parse, validate } from 'graphql';

const configPath = resolve(process.argv[2] ?? '/tmp/gavel-execution-config.json');
const operationsDir = resolve('contexts/bff/operations');
const fixture = JSON.parse(readFileSync(resolve('contexts/bff/fixtures/operations/expected-operations.json'), 'utf8'));
const config = JSON.parse(readFileSync(configPath, 'utf8'));
const schema = buildSchema(config.engineConfig.graphqlSchema);
let failed = false;

const files = readdirSync(operationsDir).filter((name) => name.endsWith('.graphql')).sort();
const expectedFiles = fixture.operations.map(({ file }) => file).sort();
if (JSON.stringify(files) !== JSON.stringify(expectedFiles)) {
  console.error(`✗ operation inventory differs: expected ${expectedFiles.join(', ')}, got ${files.join(', ')}`);
  failed = true;
}

for (const file of files) {
  const operation = readFileSync(resolve(operationsDir, file), 'utf8');
  const document = parse(operation);
  const expected = fixture.operations.find((item) => item.file === file);
  const definitions = document.definitions.filter(({ kind }) => kind === Kind.OPERATION_DEFINITION);
  const definition = definitions[0];
  const errors = validate(schema, document);
  if (definitions.length !== 1 || definition.name?.value !== expected?.name || definition.operation !== expected?.type) {
    errors.push(new Error(`expected one ${expected?.type} named ${expected?.name}`));
  }
  for (const token of fixture.forbiddenTokens) {
    if (operation.includes(token)) errors.push(new Error(`operation exposes forbidden token ${token}`));
  }
  if (errors.length === 0) {
    console.log(`✓ ${file}`);
    continue;
  }
  failed = true;
  console.error(`✗ ${file}`);
  for (const error of errors) console.error(`  ${error.message}`);
}

if (failed) process.exitCode = 1;
