use crate::{
    error::{AppError, Result},
    types::{Entity, GraphExtraction, Relation},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;

pub fn entity_id(name: &str) -> Uuid {
    let digest = Sha256::digest(name.trim().to_lowercase().as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

pub fn relation_id(source: Uuid, predicate: &str, target: Uuid) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update(predicate.trim().to_lowercase().as_bytes());
    hasher.update(target.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

pub fn parse_graph_extraction(value: &Value) -> Result<GraphExtraction> {
    let entities: Vec<Entity> = serde_json::from_value(value["entities"].clone())
        .map_err(|_| AppError::Unavailable("invalid graph extraction: entities".into()))?;
    let relations: Vec<Relation> = serde_json::from_value(value["relations"].clone())
        .map_err(|_| AppError::Unavailable("invalid graph extraction: relations".into()))?;
    if entities.len() > 20 || relations.len() > 20 {
        return Err(AppError::Unavailable("too many graph items".into()));
    }
    let mut names = HashSet::new();
    for entity in &entities {
        if entity.name.trim().is_empty() || entity.name.len() > 256 || entity.name.contains('\0') {
            return Err(AppError::Unavailable("invalid entity name".into()));
        }
        if entity.entity_type.len() > 128
            || entity.description.len() > 4000
            || entity.entity_type.contains('\0')
            || entity.description.contains('\0')
        {
            return Err(AppError::Unavailable("invalid entity fields".into()));
        }
        names.insert(entity.name.trim().to_lowercase());
    }
    for relation in &relations {
        if relation.predicate.trim().is_empty()
            || relation.predicate.len() > 256
            || relation.predicate.contains('\0')
        {
            return Err(AppError::Unavailable("invalid relation predicate".into()));
        }
        if !names.contains(&relation.source.trim().to_lowercase())
            || !names.contains(&relation.target.trim().to_lowercase())
        {
            return Err(AppError::Unavailable(
                "relation references unknown entity".into(),
            ));
        }
    }
    Ok(GraphExtraction {
        entities,
        relations,
    })
}

pub fn render_graph(
    entities: &[Value],
    relations: &[Value],
    budget: usize,
) -> (String, Vec<Value>) {
    let mut rendered = String::new();
    let mut sources = Vec::new();
    for e in entities {
        let name = e["name"].as_str().unwrap_or("");
        let id = e["id"].as_str().unwrap_or("");
        let block = format!("[entity:{id}] {name}\n");
        if rendered.len() + block.len() > budget {
            break;
        }
        rendered.push_str(&block);
        let id = e["id"].as_str().unwrap_or("");
        sources.push(json!({"citation": format!("[entity:{id}]"), "entity_id": id, "evidence":e["evidence"]}));
    }
    for r in relations {
        let fact = r["fact_text"].as_str().unwrap_or("");
        let id = r["id"].as_str().unwrap_or("");
        let block = format!("[relation:{id}] {fact}\n");
        if rendered.len() + block.len() > budget {
            break;
        }
        rendered.push_str(&block);
        let id = r["id"].as_str().unwrap_or("");
        sources.push(json!({"citation": format!("[relation:{id}]"), "relation_id": id, "evidence":r["evidence"]}));
    }
    (rendered, sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids_are_stable() {
        assert_eq!(entity_id(" Atlas "), entity_id("atlas"));
    }
    #[test]
    fn rejects_unknown_relation() {
        let value = json!({"entities":[{"name":"A","entity_type":"x","description":""}],"relations":[{"source":"A","predicate":"knows","target":"B"}]});
        assert!(parse_graph_extraction(&value).is_err());
    }
    #[test]
    fn accepts_graph() {
        let value = json!({"entities":[{"name":"A","entity_type":"x","description":""},{"name":"B","entity_type":"y","description":""}],"relations":[{"source":"A","predicate":"knows","target":"B"}]});
        assert_eq!(parse_graph_extraction(&value).unwrap().relations.len(), 1);
    }
    #[test]
    fn render_graph_respects_budget_and_cites() {
        let entities = vec![json!({"id":"e1","name":"Atlas"})];
        let relations = vec![json!({"id":"r1","fact_text":"Atlas owned_by Payments"})];
        let (text, sources) = render_graph(&entities, &relations, 1024);
        assert!(text.len() <= 1024);
        assert_eq!(sources.len(), 2);
        let (small, empty) = render_graph(&entities, &relations, 0);
        assert!(small.is_empty());
        assert!(empty.is_empty());
    }
}
