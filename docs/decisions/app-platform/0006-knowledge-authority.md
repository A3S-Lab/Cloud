# 0006: Knowledge Owns Corpus Semantics

Status: Accepted

## Context

Knowledge Bases require governed document, chunk, index, retrieval, citation,
and ingestion lifecycle. Workflow ontology models business objects and rules;
Search and vector stores are rebuildable projections.

## Decision

Knowledge owns Knowledge Base revisions, documents, source provenance,
General/Parent-child/Q&A chunks, index and retrieval policy revisions,
citations, external-Knowledge bindings, and immutable
`KnowledgePipelineRelease` identities. Metadata persists through A3S ORM;
large values use shared immutable-object references; Search/pgvector is a
rebuildable index rather than corpus truth.

Ingestion binds an exact Workflow revision and runs through Flow. Datasources
come from Sources and A3S Use, extraction through Executions/Runtime/Box,
network access through Connectors, and embedding/rerank through Inference.

## Consequences

There is no Knowledge-specific DAG engine, worker queue, object client, model
client, vector authority, or datasource package manager. Workflow ontology and
Knowledge corpus remain distinct and may be referenced together only by exact
revision.
