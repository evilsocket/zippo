use std::collections::HashMap;

use anyhow::Result;
use lazy_static::lazy_static;
use regex::Regex;

use crate::agent::{
    namespaces::Action,
    state::storage::{Storage, StorageType, CURRENT_TAG, PREVIOUS_TAG},
};

use super::Invocation;

lazy_static! {
    pub static ref XML_ATTRIBUTES_PARSER: Regex = Regex::new(r#"(?m)(([^=]+)="([^"]+)")"#).unwrap();
}

pub(crate) fn serialize_invocation(inv: &Invocation) -> String {
    let mut xml = format!("<{}", inv.action);
    if let Some(attrs) = &inv.attributes {
        for (key, value) in attrs {
            xml += &format!(" {key}=\"{value}\"");
        }
    }
    xml += &format!(
        ">{}</{}>",
        if let Some(data) = inv.payload.as_ref() {
            data
        } else {
            ""
        },
        inv.action
    );

    xml
}

#[allow(clippy::borrowed_box)]
pub(crate) fn serialize_action(action: &Box<dyn Action>) -> String {
    let mut xml = format!("<{}", action.name());

    if let Some(attrs) = action.attributes() {
        for (name, example_value) in &attrs {
            xml += &format!(" {}=\"{}\"", name, example_value);
        }
    }
    xml += ">";

    if let Some(payload) = action.example_payload() {
        xml += payload; // TODO: escape payload?
    }

    xml += &format!("</{}>", action.name());

    xml
}

pub(crate) fn serialize_storage(storage: &Storage) -> String {
    let inner = storage.get_inner().lock().unwrap();
    if inner.is_empty() {
        return "".to_string();
    }

    match storage.get_type() {
        StorageType::Tagged => {
            let mut xml: String = format!("<{}>\n", storage.get_name());

            for (key, entry) in &*inner {
                xml += &format!("  - {}={}\n", key, &entry.data);
            }

            xml += &format!("</{}>", storage.get_name());

            xml.to_string()
        }
        StorageType::Untagged => {
            let mut xml = format!("<{}>\n", storage.get_name());

            for entry in inner.values() {
                xml += &format!("  - {}\n", &entry.data);
            }

            xml += &format!("</{}>", storage.get_name());

            xml.to_string()
        }
        StorageType::Completion => {
            let mut xml = format!("<{}>\n", storage.get_name());

            for entry in inner.values() {
                xml += &format!(
                    "  - {} : {}\n",
                    &entry.data,
                    if entry.complete {
                        "COMPLETED"
                    } else {
                        "not completed"
                    }
                );
            }

            xml += &format!("</{}>", storage.get_name());

            xml.to_string()
        }
        StorageType::CurrentPrevious => {
            if let Some(current) = inner.get(CURRENT_TAG) {
                let mut str = format!("* Current {}: {}", storage.get_name(), current.data.trim());
                if let Some(prev) = inner.get(PREVIOUS_TAG) {
                    str += &format!("\n* Previous {}: {}", storage.get_name(), prev.data.trim());
                }
                str
            } else {
                "".to_string()
            }
        }
    }
}

pub(crate) fn parse_model_response(model_response: &str) -> Result<Vec<Invocation>> {
    let mut invocations = vec![];

    let model_response_size = model_response.len();
    let mut current = 0;

    // TODO: replace this with a proper xml parser
    while current < model_response_size {
        // read until < or end
        let mut ptr = &model_response[current..];
        if let Some(tag_open_idx) = ptr.find('<') {
            current += tag_open_idx;
            ptr = &ptr[tag_open_idx..];
            // read tag
            if let Some(tag_name_term_idx) = ptr.find(|c: char| c == '>' || c == ' ') {
                current += tag_name_term_idx;
                let tag_name = &ptr[1..tag_name_term_idx];
                // println!("tag_name={}", tag_name);
                if let Some(tag_close_idx) = ptr.find('>') {
                    current += tag_close_idx + tag_name.len();
                    let tag_closing = format!("</{}>", tag_name);
                    let tag_closing_idx = ptr.find(&tag_closing);

                    if let Some(tag_closing_idx) = tag_closing_idx {
                        // parse attributes if any
                        let attributes = if ptr.as_bytes()[tag_name_term_idx] == b' ' {
                            let attr_str = &ptr[tag_name_term_idx + 1..tag_close_idx];
                            let mut attrs = HashMap::new();

                            // parse as a list of key="value"
                            let iter = XML_ATTRIBUTES_PARSER.captures_iter(attr_str);
                            for caps in iter {
                                if caps.len() == 4 {
                                    let key = caps.get(2).unwrap().as_str().trim();
                                    let value = caps.get(3).unwrap().as_str().trim();
                                    attrs.insert(key.to_string(), value.to_string());
                                }
                            }

                            Some(attrs)
                        } else {
                            None
                        };

                        // parse payload if any
                        let after_tag_close = &ptr[tag_close_idx + 1..tag_closing_idx];
                        let payload = if !after_tag_close.is_empty() {
                            if after_tag_close.as_bytes()[0] != b'<' {
                                Some(after_tag_close.trim().to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        invocations.push(Invocation::new(
                            tag_name.to_string(),
                            attributes,
                            payload,
                        ));

                        continue;
                    }
                }
            }

            // just skip ahead
            current += 1;
        } else {
            // no more tags
            break;
        }
    }

    Ok(invocations)
}