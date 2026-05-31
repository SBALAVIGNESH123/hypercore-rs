# Engineering Journal — Week 12

## Monday

After comparing Postgres and SQLite, we went with SQLite because deployment was easier. The team didn't want to manage a separate database server for what's essentially a local tool.

I prefer keeping everything in a single binary when possible. Fewer moving parts means fewer things that break at 2am.

## Wednesday

Started building the memory extraction pipeline today. The idea is simple: instead of just storing chunks and embeddings like every other RAG tool, we want to extract structured memories — decisions, preferences, projects, relationships.

Decided to use keyword heuristics for v1 rather than running every chunk through an LLM. It's faster and doesn't require a model during ingestion.

## Friday

Met with Sarah to discuss the API design. We agreed to use the OpenAI-compatible format so existing SDKs work out of the box. No point inventing a new API when everyone already knows the OpenAI one.

Working on the feedback collection system. After every insight, we ask the user to rate it 1-4. This lets us measure whether the system is actually producing value or just regurgitating obvious facts.
