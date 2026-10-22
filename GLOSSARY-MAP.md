# Glossary Map

## Contexts

- [Authz](contexts/authz/GLOSSARY.md): canonical language for principals, resources, relationships and permissions.
- [Auction](contexts/auction/GLOSSARY.md): canonical language for bids, proxy pricing, auction deadlines and settlement.

Other contexts currently keep their ubiquitous language in their project README files. They can move to dedicated glossaries as the vocabulary grows.

## Relationships

- **OAuth/OIDC → Authz**: the verified `(issuer, subject)` pair becomes a canonical principal id; the identity provider does not decide resource permissions.
- **Catalog, Auction, Payments, Notify, BFF → Authz**: resource owners write access-relevant relationships; enforcement points ask SpiceDB for permissions.
- **Authz → domain contexts**: an allow decision permits a command to reach the domain but never replaces its business invariants.
