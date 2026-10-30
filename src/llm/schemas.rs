// src/llm/schemas.rs — Actor/Director 结构化输出 Schema（§7.4）
use serde_json::{json, Value};

pub fn actor_step_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "ActorStepOutput",
        "type": "object",
        "required": ["actor_action", "emotion_delta"],
        "additionalProperties": false,
        "properties": {
            "actor_action": {
                "type": "object",
                "required": ["type", "content"],
                "additionalProperties": false,
                "properties": {
                    "type": { "enum": ["dialogue", "action", "inner_monologue", "scene_move"] },
                    "content": { "type": "string", "minLength": 1, "maxLength": 800 }
                }
            },
            "emotion_delta": {
                "type": "object",
                "required": ["target_actor_id", "affinity_delta", "trust_delta"],
                "additionalProperties": false,
                "properties": {
                    "target_actor_id": { "type": "string" },
                    "affinity_delta": { "type": "integer", "minimum": -20, "maximum": 20 },
                    "trust_delta": { "type": "integer", "minimum": -20, "maximum": 20 }
                }
            },
            "director_note": { "type": "string", "maxLength": 200 }
        }
    })
}

pub fn director_schema() -> Value {
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "DirectorDecision",
        "type": "object",
        "required": ["next_actor_id", "reason"],
        "additionalProperties": false,
        "properties": {
            "next_actor_id": { "type": "string" },
            "reason": { "type": "string", "maxLength": 300 },
            "inject_scene_hint": { "type": "string", "maxLength": 200 },
            "terminate": { "type": "boolean" },
            "request_user_input": {
                "type": "object",
                "required": ["question"],
                "additionalProperties": false,
                "properties": {
                    "question": { "type": "string", "maxLength": 300 },
                    "suggestions": {
                        "type": "array",
                        "items": { "type": "string", "maxLength": 100 },
                        "maxItems": 4
                    }
                }
            }
        }
    })
}

/// JsonObject 模式的 Schema 摘要（注入 system prompt 末尾，≤800 token 的近似上限）。
pub fn compact_schema(schema: &Value) -> String {
    let mut text = serde_json::to_string(schema).unwrap_or_else(|_| "{}".to_string());
    if text.len() > 3200 {
        text.truncate(3200);
        text.push('…');
    }
    text
}
