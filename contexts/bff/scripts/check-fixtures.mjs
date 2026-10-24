import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import yaml from 'js-yaml';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const json = (path) => JSON.parse(read(path));
const failures = [];
const check = (condition, message) => {
  if (!condition) failures.push(message);
};

const composition = yaml.load(read('contexts/bff/cosmo/supergraph.yaml'));
const expected = json('contexts/bff/fixtures/composition/expected-subgraphs.json');
const actual = composition.subgraphs.map((subgraph) => ({
  name: subgraph.name,
  routingUrl: subgraph.routing_url,
  schemaFile: subgraph.schema?.file,
}));

check(composition.version === 1, 'Cosmo composition version must be 1');
check(JSON.stringify(actual) === JSON.stringify(expected.subgraphs), 'MVP subgraphs differ from the composition fixture');
for (const name of expected.deferredSubgraphs) {
  check(!actual.some((subgraph) => subgraph.name === name), `${name} must remain outside the MVP graph`);
}
for (const subgraph of actual) {
  const schemaPath = resolve(root, 'contexts/bff/cosmo', subgraph.schemaFile);
  const schema = readFileSync(schemaPath, 'utf8');
  check(schema.includes('extend schema'), `${subgraph.name} schema is not a service-owned Federation SDL`);
}

const toolchain = json('contexts/bff/cosmo/toolchain.json');
const dockerfile = read('contexts/bff/ops/Dockerfile');
check(dockerfile.includes(`wgc@${toolchain.wgcVersion}`), 'Dockerfile does not pin the declared wgc version');
check(dockerfile.includes(`FROM ${toolchain.routerImage}`), 'Dockerfile does not pin the declared Cosmo Router image');
check(dockerfile.includes(`EXECUTION_CONFIG_FILE_PATH=${toolchain.executionConfigPath}`), 'Dockerfile execution config path differs from the pin');
check(dockerfile.includes(`LISTEN_ADDR=${toolchain.listenAddress}`), 'Dockerfile listen address differs from the pin');
check(!dockerfile.includes('cargo build'), 'BFF runtime must not build the Rust bootstrap');

const operations = json('contexts/bff/fixtures/operations/expected-operations.json');
check(operations.operations.length === 2, 'MVP must expose exactly two checked client operations');
check(operations.operations.every(({ type }) => type !== 'subscription'), 'subscriptions must not enter the MVP readiness graph');

if (failures.length > 0) {
  for (const failure of failures) console.error(`✗ ${failure}`);
  process.exitCode = 1;
} else {
  console.log('✓ pinned Cosmo runtime, owner SDL composition and MVP operation fixtures');
}
