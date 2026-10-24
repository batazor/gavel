import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const json = (path) => JSON.parse(read(path));
const failures = [];
const check = (condition, message) => {
  if (!condition) failures.push(message);
};

const registry = json('contracts/event-topics.json');
const topicBootstrap = read('deploy/kind/scripts/topics.sh');
const quarantineSchema = json('contracts/schemas/processes-quarantine-v1.schema.json');
const expectedTopics = new Map([
  ['catalog-lifecycle', 'contracts/fixtures/catalog-lot-listed-v1.json'],
  ['payments-instruments', 'contracts/fixtures/payments-instrument-ready-v1.json'],
  ['auction-public', 'contracts/fixtures/auction-public-v1.json'],
  ['auction-restricted', 'contracts/fixtures/auction-restricted-v1.json'],
  ['payments-attempts', 'contracts/fixtures/payments-attempt-v1.json'],
  ['processes-quarantine', 'contracts/fixtures/processes-quarantine-v1.json'],
]);

check(registry.registryVersion === 1, 'event registry version must be 1');
check(registry.defaultCompatibility === 'BACKWARD_TRANSITIVE', 'default compatibility must be BACKWARD_TRANSITIVE');
check(registry.defaultAclPolicy === 'DENY', 'default ACL policy must deny unlisted principals');
check(registry.topics.length === expectedTopics.size, 'event registry must contain exactly the six MVP routes');
check(new Set(registry.topics.map(({ topic }) => topic)).size === registry.topics.length, 'topic names must be unique');
check(quarantineSchema.additionalProperties === false, 'quarantine schema must reject unregistered fields');
check(!JSON.stringify(quarantineSchema).toLowerCase().includes('payload'), 'quarantine schema must not represent original payload');

const camel = (field) => field.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase()).toLowerCase();
const normalizedKeys = (value, result = []) => {
  if (Array.isArray(value)) {
    for (const item of value) normalizedKeys(item, result);
  } else if (value && typeof value === 'object') {
    for (const [key, item] of Object.entries(value)) {
      result.push(key.toLowerCase());
      normalizedKeys(item, result);
    }
  }
  return result;
};

for (const topic of registry.topics) {
  const fixturePath = expectedTopics.get(topic.id);
  check(Boolean(fixturePath), `unexpected topic id: ${topic.id}`);
  check(['catalog', 'auction', 'payments', 'processes'].includes(topic.owner), `${topic.id}: owner is not explicit`);
  check(topic.schema?.version === 1, `${topic.id}: schema version must be 1`);
  check(topic.schema?.compatibility === 'BACKWARD_TRANSITIVE', `${topic.id}: compatibility must be BACKWARD_TRANSITIVE`);
  check(typeof topic.partitionKey === 'string' && topic.partitionKey.length > 0, `${topic.id}: partition key is required`);
  check(topic.acl?.produce?.length > 0 && topic.acl?.read?.length > 0, `${topic.id}: producer and reader ACLs are required`);
  check(topic.acl.produce.every((principal) => principal.startsWith('service_account:')), `${topic.id}: producer ACL must use workload principals`);
  check(topic.acl.read.every((principal) => principal.startsWith('service_account:')), `${topic.id}: reader ACL must use workload principals`);
  check(topic.allowedEventTypes?.length > 0, `${topic.id}: allowed event types are required`);
  check(read(topic.schema.source).length > 0, `${topic.id}: schema source must be readable`);
  check(topicBootstrap.includes(topic.topic), `${topic.id}: broker bootstrap misses ${topic.topic}`);

  if (!fixturePath) continue;
  const fixture = json(fixturePath);
  check(fixture.schemaVersion === topic.schema.version, `${topic.id}: fixture schemaVersion differs from registry`);
  const keys = normalizedKeys(fixture);
  for (const forbidden of topic.forbiddenFields ?? []) {
    check(!keys.includes(camel(forbidden)), `${topic.id}: fixture exposes forbidden field ${forbidden}`);
  }
}

const publicTopic = registry.topics.find(({ id }) => id === 'auction-public');
const restrictedTopic = registry.topics.find(({ id }) => id === 'auction-restricted');
const paymentsTopic = registry.topics.find(({ id }) => id === 'payments-attempts');
const catalogTopic = registry.topics.find(({ id }) => id === 'catalog-lifecycle');
const paymentInstrumentsTopic = registry.topics.find(({ id }) => id === 'payments-instruments');
check(publicTopic?.visibility === 'PUBLIC_INTEGRATION', 'Auction public route must be explicitly public integration data');
check(restrictedTopic?.visibility === 'RESTRICTED_PROCESS', 'Auction process route must be restricted');
check(paymentsTopic?.visibility === 'RESTRICTED_PROCESS', 'Payments attempt route must be restricted');
check(JSON.stringify(restrictedTopic?.acl?.read) === JSON.stringify(['service_account:processes']), 'Auction restricted route must be readable only by Processes');
check(JSON.stringify(paymentsTopic?.acl?.read) === JSON.stringify(['service_account:processes']), 'Payments attempt route must be readable only by Processes');
check(JSON.stringify(catalogTopic?.acl?.read) === JSON.stringify(['service_account:auction']), 'Catalog lifecycle route must be readable only by Auction');
check(JSON.stringify(paymentInstrumentsTopic?.acl?.read) === JSON.stringify(['service_account:auction']), 'Payments instrument route must be readable only by Auction');
check(topicBootstrap.includes('gavel-auction-owner-events-v1'), 'Auction owner-event consumer group ACL is missing');
check(topicBootstrap.includes('User:gavel-auction'), 'Auction broker principal ACL is missing');

for (const connector of ['catalog.json.tpl', 'auction-public.json.tpl', 'auction-restricted.json.tpl']) {
  const source = read(`deploy/kind/connectors/${connector}`);
  check(source.includes('ExtractField$Value'), `${connector}: connector must extract the outbox payload`);
  check(source.includes('ByteArrayConverter'), `${connector}: binary owner envelope must remain byte-exact`);
}
const paymentsConnector = read('deploy/kind/connectors/payments.json.tpl');
check(paymentsConnector.includes('io.debezium.transforms.outbox.EventRouter'), 'Payments connector must route by owner outbox topic');
check(paymentsConnector.includes('${routedByValue}'), 'Payments connector must preserve the registered topic');

const publicProto = read('contexts/auction/proto/gavel/auction/v1/auction_events.proto');
for (const forbidden of publicTopic?.forbiddenFields ?? []) {
  const declaration = new RegExp(`\\b${forbidden}\\s*=`, 'i');
  check(!declaration.test(publicProto), `Auction public protobuf exposes forbidden field ${forbidden}`);
}

const paymentProto = read('contexts/payments/proto/gavel/payments/v1/payments_events.proto');
for (const required of ['string aggregate_id = 6;', 'string causation_id = 7;']) {
  check(paymentProto.includes(required), `Payments envelope misses ${required}`);
}

const notifyProto = read('contexts/notify/proto/gavel/notify/v1/notify_service.proto');
for (const required of [
  'service NotifyOperatorService',
  'rpc GetNotification',
  'message NotificationRecord',
  'uint32 template_version = 6;',
  'string correlation_id = 7;',
  'NotificationRejectionCode code = 3;',
]) {
  check(notifyProto.includes(required), `Notify contract misses ${required}`);
}

const catalogSdl = read('contexts/catalog/graphql/schema.graphql');
const auctionSdl = read('contexts/auction/graphql/schema.graphql');
for (const required of ['type Lot @key(fields: "id")', 'lot(id: ID!): Lot']) {
  check(catalogSdl.includes(required), `Catalog Federation contract misses ${required}`);
}
for (const required of ['extend type Lot @key(fields: "id")', 'placeBid(input: PlaceBidInput!', 'viewer: AuctionViewer', 'version: Int!']) {
  check(auctionSdl.includes(required), `Auction Federation contract misses ${required}`);
}

const authzSchema = read('contexts/authz/schema/gavel.zed');
for (const required of ['relation process_worker: service_account', 'permission operate_auctions = process_worker', 'permission operate_payments = process_worker', 'permission send_notifications = process_worker']) {
  check(authzSchema.includes(required), `SpiceDB contract misses ${required}`);
}

const pins = json('contracts/bsr-dependencies.json');
const bufGenerate = read('buf.gen.yaml');
for (const plugin of pins.plugins) check(bufGenerate.includes(plugin), `buf.gen.yaml misses pinned plugin ${plugin}`);
for (const dependency of pins.generatedSdks) {
  const manifest = read(dependency.consumerManifest);
  for (const pkg of dependency.packages) {
    check(/^\d+\.\d+\.\d+-\d+-[0-9a-f]{12}\.\d+$/.test(pkg.version), `${pkg.name}: Generated SDK version must embed a module commit`);
    check(manifest.includes(`package = "${pkg.name}"`), `${dependency.consumerManifest}: missing ${pkg.name}`);
    check(manifest.includes(`version = "=${pkg.version}"`), `${dependency.consumerManifest}: ${pkg.name} must use an exact version`);
  }
  if (dependency.provenanceLock) {
    const lock = json(dependency.provenanceLock);
    check(lock.module === dependency.module, `${dependency.provenanceLock}: module provenance mismatch`);
    for (const pkg of dependency.packages) {
      const versions = Object.values(lock.generatedSdks).map(({ version }) => version);
      check(versions.includes(pkg.version), `${dependency.provenanceLock}: missing ${pkg.version}`);
    }
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(`✗ ${failure}`);
  process.exitCode = 1;
} else {
  console.log('✓ shared event registry, privacy boundaries, owner contracts and Buf/BSR pins');
}
