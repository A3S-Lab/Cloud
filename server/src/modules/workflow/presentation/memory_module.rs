use std::collections::HashMap;
use std::sync::Arc;

use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, Module, ModuleRef,
    ProviderDefinition, Result,
};
use a3s_memory::{MemoryItem, MemoryStore, MemoryType};
use serde::Deserialize;

#[derive(Clone)]
pub struct MemoryModule {
    store: Arc<dyn MemoryStore>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreRequest {
    content: String,
    #[serde(default = "default_importance")]
    importance: f32,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_memory_type")]
    memory_type: MemoryType,
    #[serde(default)]
    metadata: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchRequest {
    #[serde(default)]
    query: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

const fn default_importance() -> f32 {
    0.5
}

const fn default_memory_type() -> MemoryType {
    MemoryType::Episodic
}

const fn default_limit() -> usize {
    10
}

impl MemoryModule {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }

    fn controller(store: Arc<dyn MemoryStore>) -> Result<ControllerDefinition> {
        let store_service = Arc::clone(&store);
        let search_service = Arc::clone(&store);
        let retrieve_service = Arc::clone(&store);
        ControllerDefinition::new("/api/v1")?
            .post("/memories:store", move |request: BootRequest| {
                let store = Arc::clone(&store_service);
                async move {
                    let payload = request.json::<StoreRequest>()?;
                    if payload.content.trim().is_empty() {
                        return Err(BootError::UnprocessableEntity(
                            "memory content must not be empty".to_string(),
                        ));
                    }
                    let mut item = MemoryItem::new(payload.content)
                        .with_importance(payload.importance)
                        .with_tags(payload.tags)
                        .with_type(payload.memory_type);
                    item.metadata = payload.metadata;
                    let item = store.store_and_return(item).await.map_err(memory_error)?;
                    BootResponse::json_with_status(201, &item)
                }
            })?
            .post("/memories:search", move |request: BootRequest| {
                let store = Arc::clone(&search_service);
                async move {
                    let payload = request.json::<SearchRequest>()?;
                    let items = if payload.tags.is_empty() {
                        store.search(&payload.query, payload.limit).await
                    } else {
                        store.search_by_tags(&payload.tags, payload.limit).await
                    }
                    .map_err(memory_error)?;
                    BootResponse::json(&items)
                }
            })?
            .get("/memories/{id}", move |request: BootRequest| {
                let store = Arc::clone(&retrieve_service);
                async move {
                    let id = required_param(&request, "id")?;
                    let item = store
                        .retrieve(&id)
                        .await
                        .map_err(memory_error)?
                        .ok_or_else(|| BootError::NotFound(format!("memory {id}")))?;
                    BootResponse::json(&item)
                }
            })?
            .delete("/memories/{id}", move |request: BootRequest| {
                let store = Arc::clone(&store);
                async move {
                    let id = required_param(&request, "id")?;
                    store.delete(&id).await.map_err(memory_error)?;
                    Ok(BootResponse::no_content())
                }
            })
    }
}

impl Module for MemoryModule {
    fn name(&self) -> &'static str {
        "postgres-memory"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(Vec::new())
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Self::controller(Arc::clone(&self.store))?])
    }
}

fn required_param(request: &BootRequest, name: &str) -> Result<String> {
    request
        .param(name)
        .map(str::to_string)
        .ok_or_else(|| BootError::BadRequest(format!("missing path parameter {name}")))
}

fn memory_error(error: anyhow::Error) -> BootError {
    BootError::Internal(error.to_string())
}
