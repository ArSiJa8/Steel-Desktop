use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, Value};

const IGNORED_WORLD_SECTIONS: [&str; 3] = ["storage", "player_storage", "domains"];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingField {
    pub path: Vec<String>,
    pub section: String,
    pub label: String,
    pub comment: Option<String>,
    pub kind: String,
    pub value: JsonValue,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSettingsResponse {
    pub world_settings: Vec<SettingField>,
    pub server_settings: Vec<SettingField>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingPayload {
    pub file: String,
    pub path: Vec<String>,
    pub kind: String,
    pub value: JsonValue,
}

fn extract_comment(table: &Table, key: &str) -> Option<String> {
    let key_ref = table.key(key)?;
    let prefix = key_ref.leaf_decor().prefix()?.as_str()?;
    let lines: Vec<String> = prefix
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('#') && !l.starts_with("#:schema"))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" "))
    }
}

fn collect_value(
    val: &Value,
    path: Vec<String>,
    section: &str,
    label: &str,
    comment: Option<String>,
    fields: &mut Vec<SettingField>,
) {
    match val {
        Value::Boolean(b) => fields.push(SettingField {
            path,
            section: section.to_string(),
            label: label.to_string(),
            comment,
            kind: "boolean".to_string(),
            value: JsonValue::Bool(*b.value()),
        }),
        Value::Integer(i) => fields.push(SettingField {
            path,
            section: section.to_string(),
            label: label.to_string(),
            comment,
            kind: "integer".to_string(),
            value: JsonValue::from(*i.value()),
        }),
        Value::Float(f) => {
            if let Some(num) = serde_json::Number::from_f64(*f.value()) {
                fields.push(SettingField {
                    path,
                    section: section.to_string(),
                    label: label.to_string(),
                    comment,
                    kind: "float".to_string(),
                    value: JsonValue::Number(num),
                });
            }
        }
        Value::String(s) => fields.push(SettingField {
            path,
            section: section.to_string(),
            label: label.to_string(),
            comment,
            kind: "string".to_string(),
            value: JsonValue::String(s.value().clone()),
        }),
        Value::InlineTable(inline) => {
            for (k, v) in inline.iter() {
                let mut child_path = path.clone();
                child_path.push(k.to_string());
                let child_label = format!("{label}.{k}");
                collect_value(v, child_path, section, &child_label, None, fields);
            }
        }
        Value::Array(arr) => {
            for (idx, v) in arr.iter().enumerate() {
                let mut child_path = path.clone();
                child_path.push(idx.to_string());
                let child_label = format!("{label}[{idx}]");
                collect_value(v, child_path, section, &child_label, None, fields);
            }
        }
        _ => {}
    }
}

fn collect_table(
    table: &Table,
    prefix_path: &[String],
    section: &str,
    ignored_root_keys: &[&str],
    fields: &mut Vec<SettingField>,
) {
    for (key, item) in table.iter() {
        if prefix_path.is_empty() && ignored_root_keys.contains(&key) {
            continue;
        }

        let mut next_path = prefix_path.to_vec();
        next_path.push(key.to_string());
        let comment = extract_comment(table, key);

        match item {
            Item::Value(val) => {
                collect_value(val, next_path, section, key, comment, fields);
            }
            Item::Table(sub_table) => {
                let next_section = if section == "General" {
                    key.to_string()
                } else {
                    format!("{section}.{key}")
                };
                collect_table(sub_table, &next_path, &next_section, &[], fields);
            }
            Item::ArrayOfTables(aot) => {
                let base_section = if section == "General" {
                    key.to_string()
                } else {
                    format!("{section}.{key}")
                };
                for (idx, sub_table) in aot.iter().enumerate() {
                    let mut idx_path = next_path.clone();
                    idx_path.push(idx.to_string());
                    let idx_section = format!("{base_section} #{idx_plus}", idx_plus = idx + 1);
                    collect_table(sub_table, &idx_path, &idx_section, &[], fields);
                }
            }
            _ => {}
        }
    }
}

fn read_toml_fields(path: &Path, ignored_root_keys: &[&str]) -> Result<Vec<SettingField>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let doc: DocumentMut = content
        .parse()
        .map_err(|e| format!("Failed to parse {}: {e}", path.display()))?;

    let mut fields = Vec::new();
    collect_table(doc.as_table(), &[], "General", ignored_root_keys, &mut fields);
    Ok(fields)
}

pub fn load_all_settings(folder_path: &Path) -> Result<ConfigSettingsResponse, String> {
    let config_dir = folder_path.join("config");
    let world_settings = read_toml_fields(&config_dir.join("worlds.toml"), &IGNORED_WORLD_SECTIONS)?;
    let server_settings = read_toml_fields(&config_dir.join("config.toml"), &[])?;

    Ok(ConfigSettingsResponse {
        world_settings,
        server_settings,
    })
}

fn json_to_toml_value(kind: &str, value: &JsonValue) -> Result<Value, String> {
    match kind {
        "boolean" => {
            let b = value
                .as_bool()
                .ok_or_else(|| "Expected boolean value.".to_string())?;
            Ok(Value::from(b))
        }
        "integer" => {
            let i = value
                .as_i64()
                .or_else(|| value.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
                .ok_or_else(|| "Expected integer value.".to_string())?;
            Ok(Value::from(i))
        }
        "float" => {
            let f = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
                .ok_or_else(|| "Expected float value.".to_string())?;
            Ok(Value::from(f))
        }
        "string" => {
            let s = value
                .as_str()
                .ok_or_else(|| "Expected string value.".to_string())?;
            Ok(Value::from(s))
        }
        _ => Err(format!("Unsupported setting kind: {kind}")),
    }
}

fn update_value_in_item(item: &mut Item, path: &[String], new_val: Value) -> Result<(), String> {
    if path.is_empty() {
        let existing = item
            .as_value_mut()
            .ok_or_else(|| "Target is not a scalar value.".to_string())?;
        let decor = existing.decor().clone();
        *existing = new_val;
        *existing.decor_mut() = decor;
        return Ok(());
    }

    let head = &path[0];
    let rest = &path[1..];

    match item {
        Item::Table(table) => {
            let child = table
                .get_mut(head)
                .ok_or_else(|| format!("Key '{head}' not found."))?;
            update_value_in_item(child, rest, new_val)
        }
        Item::ArrayOfTables(aot) => {
            let idx: usize = head
                .parse()
                .map_err(|_| format!("Invalid array index '{head}'."))?;
            let sub_table = aot
                .get_mut(idx)
                .ok_or_else(|| format!("Array index {idx} out of bounds."))?;
            if rest.is_empty() {
                return Err("Cannot replace an entire table with a scalar.".to_string());
            }
            let child = sub_table
                .get_mut(&rest[0])
                .ok_or_else(|| format!("Key '{}' not found.", rest[0]))?;
            update_value_in_item(child, &rest[1..], new_val)
        }
        Item::Value(Value::InlineTable(inline)) => {
            let val = inline
                .get_mut(head)
                .ok_or_else(|| format!("Inline key '{head}' not found."))?;
            update_value_in_toml_value(val, rest, new_val)
        }
        Item::Value(Value::Array(arr)) => {
            let idx: usize = head
                .parse()
                .map_err(|_| format!("Invalid array index '{head}'."))?;
            let val = arr
                .get_mut(idx)
                .ok_or_else(|| format!("Array index {idx} out of bounds."))?;
            update_value_in_toml_value(val, rest, new_val)
        }
        _ => Err(format!("Cannot traverse into '{head}'.")),
    }
}

fn update_value_in_toml_value(
    val: &mut Value,
    path: &[String],
    new_val: Value,
) -> Result<(), String> {
    if path.is_empty() {
        let decor = val.decor().clone();
        *val = new_val;
        *val.decor_mut() = decor;
        return Ok(());
    }

    let head = &path[0];
    let rest = &path[1..];

    match val {
        Value::InlineTable(inline) => {
            let child = inline
                .get_mut(head)
                .ok_or_else(|| format!("Inline key '{head}' not found."))?;
            update_value_in_toml_value(child, rest, new_val)
        }
        Value::Array(arr) => {
            let idx: usize = head
                .parse()
                .map_err(|_| format!("Invalid array index '{head}'."))?;
            let child = arr
                .get_mut(idx)
                .ok_or_else(|| format!("Array index {idx} out of bounds."))?;
            update_value_in_toml_value(child, rest, new_val)
        }
        _ => Err(format!("Cannot traverse into '{head}'.")),
    }
}

pub fn save_setting(folder_path: &Path, payload: UpdateSettingPayload) -> Result<(), String> {
    let file_name = match payload.file.as_str() {
        "worlds" => "worlds.toml",
        "config" => "config.toml",
        other => return Err(format!("Unknown config file: {other}")),
    };

    let target_path = folder_path.join("config").join(file_name);
    let content = std::fs::read_to_string(&target_path)
        .map_err(|e| format!("Failed to read {}: {e}", target_path.display()))?;
    let mut doc: DocumentMut = content
        .parse()
        .map_err(|e| format!("Failed to parse {}: {e}", target_path.display()))?;

    let new_val = json_to_toml_value(&payload.kind, &payload.value)?;
    if payload.path.is_empty() {
        return Err("Setting path cannot be empty.".to_string());
    }

    let root_key = &payload.path[0];
    let child = doc
        .get_mut(root_key)
        .ok_or_else(|| format!("Key '{root_key}' not found."))?;
    update_value_in_item(child, &payload.path[1..], new_val)?;

    std::fs::write(&target_path, doc.to_string())
        .map_err(|e| format!("Failed to write {}: {e}", target_path.display()))?;
    Ok(())
}
